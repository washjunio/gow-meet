use crate::asr::{clean_transcript, Transcriber};
use crate::segmenter::{RmsDetector, SegmentEvent, Segmenter, SegmenterConfig};
use crate::session::{Line, SessionWriter, Speaker, SpeakerSource};
use crate::translate::Translator;
use anyhow::{Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineEvent {
    Partial { id: String, speaker: Speaker, t0: u64, t1: u64, text: String, translation: Option<String> },
    Final(Line),
    Translated(Line),
    Lag { ms: u64 },
    Error(String),
    /// Parcial exibida cuja fala final foi descartada (vazia/marcador).
    Dropped { id: String },
}

pub trait Sink: Send + Sync {
    fn emit(&self, ev: PipelineEvent);
}

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub segmenter: SegmenterConfig,
    pub vad_threshold: f32,
    pub partial_translate_every_ms: u64,
    pub lag_warn_ms: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { segmenter: SegmenterConfig::default(), vad_threshold: 0.01,
               partial_translate_every_ms: 1_500, lag_warn_ms: 5_000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsrJob {
    pub source: SpeakerSource,
    pub seq: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub audio: Vec<f32>,
    pub is_final: bool,
}

pub fn line_id(source: SpeakerSource, seq: u64) -> String {
    match source { SpeakerSource::Mic => format!("m{seq}"), SpeakerSource::System => format!("s{seq}") }
}

fn speaker_for(source: SpeakerSource) -> Speaker {
    match source { SpeakerSource::Mic => Speaker::mic(), SpeakerSource::System => Speaker::system() }
}

/// Sob atraso: mantém todos os finais e só a parcial mais recente de cada fonte,
/// descartando parciais cuja fala já fechou no mesmo lote.
pub fn coalesce(batch: Vec<AsrJob>) -> Vec<AsrJob> {
    let finals: HashSet<(SpeakerSource, u64)> =
        batch.iter().filter(|j| j.is_final).map(|j| (j.source, j.seq)).collect();
    let mut last: HashMap<SpeakerSource, usize> = HashMap::new();
    for (i, j) in batch.iter().enumerate() {
        if !j.is_final && !finals.contains(&(j.source, j.seq)) { last.insert(j.source, i); }
    }
    batch.into_iter().enumerate()
        .filter(|(i, j)| j.is_final || last.get(&j.source) == Some(i))
        .map(|(_, j)| j)
        .collect()
}

/// Gravação com nova tentativa: falas que falharam ficam em memória e são
/// regravadas antes da próxima.
struct Store { writer: SessionWriter, pending: Vec<Line> }

impl Store {
    fn append(&mut self, line: &Line, sink: &dyn Sink) {
        self.pending.push(line.clone());
        while let Some(l) = self.pending.first() {
            if let Err(e) = self.writer.append(l) {
                sink.emit(PipelineEvent::Error(format!("falha ao gravar transcrição: {e}")));
                return;
            }
            self.pending.remove(0);
        }
    }
}

/// Parciais ainda abertas: id -> t1 da parcial mais recente exibida. Ausente
/// significa fechada (final/descartada) ou nunca exibida. Quem emite eventos de
/// um id o faz com este lock, o que garante a ordem por id.
type OpenPartials = Arc<Mutex<HashMap<String, u64>>>;

enum TJob {
    Final(Line),
    Partial { id: String, speaker: Speaker, t0: u64, t1: u64, text: String },
}

pub struct Pipeline {
    cfg: PipelineConfig,
    segmenters: Mutex<HashMap<SpeakerSource, Segmenter>>,
    fed: Arc<Mutex<HashMap<SpeakerSource, u64>>>,
    asr_tx: Option<Sender<AsrJob>>,
    asr: Option<JoinHandle<()>>,
    tr: Option<JoinHandle<()>>,
    store: Arc<Mutex<Option<Store>>>,
}

impl Pipeline {
    pub fn start(
        cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>,
        writer: SessionWriter, sink: Arc<dyn Sink>,
    ) -> Self {
        let (asr_tx, asr_rx) = unbounded::<AsrJob>();
        let (tr_tx, tr_rx) = unbounded::<TJob>();
        let store = Arc::new(Mutex::new(Some(Store { writer, pending: vec![] })));
        let fed = Arc::new(Mutex::new(HashMap::new()));
        let open: OpenPartials = Arc::new(Mutex::new(HashMap::new()));
        let asr_h = {
            let (store, sink, fed, warn, open) =
                (store.clone(), sink.clone(), fed.clone(), cfg.lag_warn_ms, open.clone());
            std::thread::Builder::new().name("asr".into())
                .spawn(move || asr_loop(asr_rx, asr, tr_tx, store, sink, fed, warn, open)).expect("thread asr")
        };
        let tr_h = {
            let (store, every) = (store.clone(), Duration::from_millis(cfg.partial_translate_every_ms));
            std::thread::Builder::new().name("translate".into())
                .spawn(move || translate_loop(tr_rx, translator, store, sink, every, open)).expect("thread translate")
        };
        Self { cfg, segmenters: Mutex::new(HashMap::new()), fed, asr_tx: Some(asr_tx),
               asr: Some(asr_h), tr: Some(tr_h), store }
    }

    pub fn feed(&self, source: SpeakerSource, samples: &[f32]) {
        let events = {
            let mut segs = self.segmenters.lock().unwrap();
            let seg = segs.entry(source).or_insert_with(|| Segmenter::new(
                self.cfg.segmenter.clone(), Box::new(RmsDetector { threshold: self.cfg.vad_threshold })));
            seg.push(samples)
        };
        *self.fed.lock().unwrap().entry(source).or_insert(0) += samples.len() as u64;
        self.send(source, events);
    }

    fn send(&self, source: SpeakerSource, events: Vec<SegmentEvent>) {
        let Some(tx) = &self.asr_tx else { return };
        for ev in events {
            let job = match ev {
                SegmentEvent::Partial { seq, start_ms, audio } => {
                    let end_ms = start_ms + audio.len() as u64 / 16;
                    AsrJob { source, seq, start_ms, end_ms, audio, is_final: false }
                }
                SegmentEvent::Final { seq, start_ms, end_ms, audio } =>
                    AsrJob { source, seq, start_ms, end_ms, audio, is_final: true },
            };
            let _ = tx.send(job);
        }
    }

    pub fn finish(mut self, ended_ms: u64) -> Result<PathBuf> {
        let flushed: Vec<(SpeakerSource, Vec<SegmentEvent>)> = self.segmenters.lock().unwrap()
            .iter_mut().map(|(s, seg)| (*s, seg.flush())).collect();
        for (s, ev) in flushed { self.send(s, ev); }
        self.asr_tx.take();
        if let Some(h) = self.asr.take() { let _ = h.join(); }
        if let Some(h) = self.tr.take() { let _ = h.join(); }
        let store = self.store.lock().unwrap().take().context("sessão já encerrada")?;
        let mut writer = store.writer;
        for l in &store.pending { writer.append(l)?; }
        writer.finish(ended_ms)
    }
}

/// Avisos de pânico: genéricos, nunca com o conteúdo da fala.
pub const ASR_PANIC_MESSAGE: &str =
    "O reconhecimento de fala parou por um erro interno. Encerre e inicie a reunião de novo.";
pub const TRANSLATE_PANIC_MESSAGE: &str =
    "A tradução parou por um erro interno; as falas seguem só no original. Encerre e inicie a reunião de novo.";

/// Roda `f` isolando um pânico do Whisper/CTranslate2; `None` se entrou em pânico.
/// Depois de um pânico o modelo não é chamado de novo (estado interno incerto).
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

fn asr_loop(
    rx: Receiver<AsrJob>, mut asr: Box<dyn Transcriber>, tr_tx: Sender<TJob>,
    store: Arc<Mutex<Option<Store>>>, sink: Arc<dyn Sink>,
    fed: Arc<Mutex<HashMap<SpeakerSource, u64>>>, lag_warn_ms: u64, open: OpenPartials,
) {
    let mut dead = false;
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        for job in coalesce(batch) {
            let fed_ms = fed.lock().unwrap().get(&job.source).copied().unwrap_or(0) / 16;
            let lag = fed_ms.saturating_sub(job.end_ms);
            if lag > lag_warn_ms { sink.emit(PipelineEvent::Lag { ms: lag }); }
            let id = line_id(job.source, job.seq);
            // Fala final sem texto aproveitável: remove a parcial já exibida, se houver.
            let drop_final = |sink: &dyn Sink| {
                if job.is_final {
                    let mut o = open.lock().unwrap();
                    if o.remove(&id).is_some() { sink.emit(PipelineEvent::Dropped { id: id.clone() }); }
                }
            };
            // Transcritor morto: segue drenando a fila para não acumular áudio.
            if dead {
                drop_final(sink.as_ref());
                continue;
            }
            let raw = match guarded(|| asr.transcribe(&job.audio)) {
                None => {
                    dead = true;
                    tracing::error!("pânico na transcrição; reconhecimento desligado nesta sessão");
                    sink.emit(PipelineEvent::Error(ASR_PANIC_MESSAGE.into()));
                    drop_final(sink.as_ref());
                    continue;
                }
                Some(Ok(t)) => t,
                Some(Err(e)) => {
                    tracing::warn!(source = ?job.source, seq = job.seq, "falha na transcrição: {e}");
                    sink.emit(PipelineEvent::Error(format!("falha na transcrição: {e}")));
                    drop_final(sink.as_ref());
                    continue;
                }
            };
            let Some(text) = clean_transcript(&raw) else {
                drop_final(sink.as_ref());
                continue;
            };
            let speaker = speaker_for(job.source);
            if job.is_final {
                let line = Line { id, t0: job.start_ms, t1: job.end_ms, speaker, text,
                                  translation: None, translation_error: false };
                if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                {
                    let mut o = open.lock().unwrap();
                    o.remove(&line.id);
                    sink.emit(PipelineEvent::Final(line.clone()));
                }
                let _ = tr_tx.send(TJob::Final(line));
            } else {
                {
                    let mut o = open.lock().unwrap();
                    o.insert(id.clone(), job.end_ms);
                    sink.emit(PipelineEvent::Partial { id: id.clone(), speaker: speaker.clone(),
                        t0: job.start_ms, t1: job.end_ms, text: text.clone(), translation: None });
                }
                let _ = tr_tx.send(TJob::Partial { id, speaker, t0: job.start_ms, t1: job.end_ms, text });
            }
        }
    }
}

fn translate_loop(
    rx: Receiver<TJob>, mut tr: Box<dyn Translator>, store: Arc<Mutex<Option<Store>>>,
    sink: Arc<dyn Sink>, every: Duration, open: OpenPartials,
) {
    let mut last_partial: HashMap<SpeakerSource, Instant> = HashMap::new();
    let mut dead = false;
    // Traduz isolando pânicos: o primeiro desliga a tradução e avisa uma vez.
    let mut translate = |text: &str| -> Option<Result<String>> {
        if dead { return None; }
        let r = guarded(|| tr.translate(text));
        if r.is_none() {
            dead = true;
            tracing::error!("pânico na tradução; tradução desligada nesta sessão");
            sink.emit(PipelineEvent::Error(TRANSLATE_PANIC_MESSAGE.into()));
        }
        r
    };
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        let final_ids: HashSet<String> = batch.iter()
            .filter_map(|j| match j { TJob::Final(l) => Some(l.id.clone()), _ => None }).collect();
        let mut partials: HashMap<SpeakerSource, TJob> = HashMap::new();
        for job in batch {
            match job {
                TJob::Final(mut line) => {
                    match translate(&line.text) {
                        Some(Ok(t)) => line.translation = Some(t),
                        Some(Err(e)) => {
                            tracing::warn!(id = %line.id, "falha na tradução: {e}");
                            line.translation_error = true;
                        }
                        None => line.translation_error = true,
                    }
                    if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                    sink.emit(PipelineEvent::Translated(line));
                }
                TJob::Partial { ref id, ref speaker, .. } if !final_ids.contains(id) => {
                    partials.insert(speaker.source, job);
                }
                TJob::Partial { .. } => {}
            }
        }
        for (src, job) in partials {
            if last_partial.get(&src).is_some_and(|t| t.elapsed() < every) { continue; }
            let TJob::Partial { id, speaker, t0, t1, text } = job else { continue };
            if let Some(Ok(t)) = translate(&text) {
                last_partial.insert(src, Instant::now());
                // Só emite se a parcial ainda é a mais recente e a fala segue aberta.
                let o = open.lock().unwrap();
                if o.get(&id).is_some_and(|&latest| t1 >= latest) {
                    sink.emit(PipelineEvent::Partial { id, speaker, t0, t1, text, translation: Some(t) });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{replay, Meta};

    const MS: usize = 16;
    fn tone(ms: usize) -> Vec<f32> { (0..ms * MS).map(|i| 0.3 * (i as f32 * 0.17).sin()).collect() }
    fn silence(ms: usize) -> Vec<f32> { vec![0.0; ms * MS] }

    struct FakeAsr(&'static str);
    impl Transcriber for FakeAsr {
        fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok(self.0.to_string()) }
    }
    struct Prefix;
    impl Translator for Prefix {
        fn translate(&mut self, t: &str) -> Result<String> { Ok(format!("pt: {t}")) }
    }
    struct Broken;
    impl Translator for Broken {
        fn translate(&mut self, _t: &str) -> Result<String> { anyhow::bail!("falhou") }
    }
    #[derive(Default)]
    struct Collect(Mutex<Vec<PipelineEvent>>);
    impl Sink for Collect {
        fn emit(&self, ev: PipelineEvent) { self.0.lock().unwrap().push(ev); }
    }

    fn run(asr: &'static str, tr: Box<dyn Translator>, feed: impl FnOnce(&Pipeline))
        -> (tempfile::TempDir, Vec<PipelineEvent>, Vec<Line>) {
        run_with(Box::new(FakeAsr(asr)), tr, feed)
    }

    fn run_with(asr: Box<dyn Transcriber>, tr: Box<dyn Translator>, feed: impl FnOnce(&Pipeline))
        -> (tempfile::TempDir, Vec<PipelineEvent>, Vec<Line>) {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let sink = Arc::new(Collect::default());
        let p = Pipeline::start(PipelineConfig::default(), asr, tr, writer, sink.clone());
        feed(&p);
        let md = p.finish(10_000).unwrap();
        let lines = replay(&md.parent().unwrap().join("lines.jsonl")).unwrap();
        let events = sink.0.lock().unwrap().clone();
        (root, events, lines)
    }

    #[test]
    fn final_then_translation_with_same_id() {
        let (_d, ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat());
        });
        assert!(ev.iter().any(|e| matches!(e, PipelineEvent::Final(l) if l.id == "s0" && l.translation.is_none())));
        assert!(ev.iter().any(|e| matches!(e, PipelineEvent::Translated(l)
            if l.id == "s0" && l.translation.as_deref() == Some("pt: Hello team."))));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].translation.as_deref(), Some("pt: Hello team."));
        assert_eq!(lines[0].speaker.label, "Participante");
    }

    #[test]
    fn failed_translation_keeps_original() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Broken), |p| {
            p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat());
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].translation_error);
        assert_eq!(lines[0].text, "Hello team.");
    }

    #[test]
    fn simultaneous_sources_get_distinct_ids() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            let a = [tone(1_500), silence(1_000)].concat();
            for (m, s) in a.chunks(480).zip(a.chunks(480)) {
                p.feed(SpeakerSource::Mic, m);
                p.feed(SpeakerSource::System, s);
            }
        });
        let mut ids: Vec<_> = lines.iter().map(|l| (l.id.clone(), l.speaker.label.clone())).collect();
        ids.sort();
        assert_eq!(ids, vec![("m0".into(), "Você".into()), ("s0".into(), "Participante".into())]);
    }

    struct PanicAsr;
    impl Transcriber for PanicAsr {
        fn transcribe(&mut self, _a: &[f32]) -> Result<String> { panic!("falha simulada no transcritor") }
    }
    struct PanicTranslator;
    impl Translator for PanicTranslator {
        fn translate(&mut self, _t: &str) -> Result<String> { panic!("falha simulada no tradutor") }
    }

    fn two_utterances(p: &Pipeline) {
        for _ in 0..2 { p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat()); }
    }

    fn errors(ev: &[PipelineEvent]) -> Vec<String> {
        ev.iter().filter_map(|e| match e { PipelineEvent::Error(m) => Some(m.clone()), _ => None }).collect()
    }

    #[test]
    fn asr_panic_emits_one_generic_error() {
        let (_d, ev, lines) = run_with(Box::new(PanicAsr), Box::new(Prefix), two_utterances);
        assert_eq!(errors(&ev), vec![ASR_PANIC_MESSAGE.to_string()]);
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(_))));
        assert!(lines.is_empty());
    }

    #[test]
    fn translator_panic_emits_one_error_and_keeps_originals() {
        let (_d, ev, lines) = run_with(Box::new(FakeAsr("Hello team.")), Box::new(PanicTranslator),
                                       two_utterances);
        assert_eq!(errors(&ev), vec![TRANSLATE_PANIC_MESSAGE.to_string()]);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.translation_error && l.text == "Hello team."));
        assert_eq!(ev.iter().filter(|e| matches!(e, PipelineEvent::Translated(_))).count(), 2);
    }

    #[test]
    fn hallucination_produces_no_line() {
        let (_d, ev, lines) = run("[BLANK_AUDIO]", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat());
        });
        assert!(lines.is_empty());
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(_))));
    }

    struct ChanSink(Mutex<Vec<PipelineEvent>>, crossbeam_channel::Sender<PipelineEvent>);
    impl Sink for ChanSink {
        fn emit(&self, ev: PipelineEvent) {
            self.0.lock().unwrap().push(ev.clone());
            let _ = self.1.send(ev);
        }
    }

    /// Alimenta fala em pedaços, espera (com limite) uma parcial de `id`, e só
    /// então fecha a fala com silêncio. Devolve os eventos e as linhas gravadas.
    fn run_partial_then_close(asr: Box<dyn Transcriber>, id: &str, on_partial: impl FnOnce())
        -> (tempfile::TempDir, Vec<PipelineEvent>, Vec<Line>) {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = Arc::new(ChanSink(Mutex::new(vec![]), tx));
        let p = Pipeline::start(PipelineConfig::default(), asr, Box::new(Prefix), writer, sink.clone());
        for c in tone(3_500).chunks(480) { p.feed(SpeakerSource::System, c); }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(PipelineEvent::Partial { id: pid, .. }) if pid == id => break,
                Ok(_) => {}
                Err(_) => panic!("parcial não chegou a tempo"),
            }
        }
        on_partial();
        p.feed(SpeakerSource::System, &silence(1_000));
        let md = p.finish(10_000).unwrap();
        let lines = replay(&md.parent().unwrap().join("lines.jsonl")).unwrap();
        let events = sink.0.lock().unwrap().clone();
        (root, events, lines)
    }

    #[test]
    fn long_speech_emits_partial_before_final() {
        let (_d, ev, lines) = run_partial_then_close(Box::new(FakeAsr("Hello team.")), "s0", || {});
        let first_partial = ev.iter().position(|e| matches!(e, PipelineEvent::Partial { id, .. } if id == "s0"));
        let first_final = ev.iter().position(|e| matches!(e, PipelineEvent::Final(_)));
        assert!(first_partial.is_some() && first_final.is_some() && first_partial < first_final);
        assert_eq!(lines.len(), 1);
    }

    /// Antes do gatilho devolve texto; depois, marcador (ou erro, se `fail`).
    struct Switch { flag: Arc<std::sync::atomic::AtomicBool>, fail: bool }
    impl Transcriber for Switch {
        fn transcribe(&mut self, _a: &[f32]) -> Result<String> {
            if !self.flag.load(std::sync::atomic::Ordering::SeqCst) { return Ok("Hello team.".into()); }
            if self.fail { anyhow::bail!("falhou") }
            Ok("[BLANK_AUDIO]".into())
        }
    }

    #[test]
    fn discarded_final_after_partial_emits_dropped() {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let f2 = flag.clone();
        let (_d, ev, lines) = run_partial_then_close(Box::new(Switch { flag, fail: false }), "s0",
            move || f2.store(true, std::sync::atomic::Ordering::SeqCst));
        assert!(lines.is_empty());
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(_))));
        let p = ev.iter().position(|e| matches!(e, PipelineEvent::Partial { id, .. } if id == "s0")).unwrap();
        let d = ev.iter().position(|e| matches!(e, PipelineEvent::Dropped { id } if id == "s0")).unwrap();
        assert!(p < d);
        assert_eq!(ev.iter().filter(|e| matches!(e, PipelineEvent::Dropped { .. })).count(), 1);
    }

    #[test]
    fn failed_final_after_partial_emits_dropped() {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let f2 = flag.clone();
        let (_d, ev, lines) = run_partial_then_close(Box::new(Switch { flag, fail: true }), "s0",
            move || f2.store(true, std::sync::atomic::Ordering::SeqCst));
        assert!(lines.is_empty());
        assert!(ev.iter().any(|e| matches!(e, PipelineEvent::Error(_))));
        let p = ev.iter().position(|e| matches!(e, PipelineEvent::Partial { id, .. } if id == "s0")).unwrap();
        let d = ev.iter().position(|e| matches!(e, PipelineEvent::Dropped { id } if id == "s0")).unwrap();
        assert!(p < d);
    }

    /// Tradutor que bloqueia na primeira chamada até ser liberado pelo teste.
    struct Gate { entered: crossbeam_channel::Sender<()>, release: Receiver<()>, first: bool }
    impl Translator for Gate {
        fn translate(&mut self, t: &str) -> Result<String> {
            if std::mem::take(&mut self.first) {
                let _ = self.entered.send(());
                let _ = self.release.recv_timeout(Duration::from_secs(10));
            }
            Ok(format!("pt: {t}"))
        }
    }

    fn gated() -> (tempfile::TempDir, Pipeline, Arc<ChanSink>, Receiver<PipelineEvent>,
                   Receiver<()>, Sender<()>) {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = Arc::new(ChanSink(Mutex::new(vec![]), tx));
        let (etx, erx) = crossbeam_channel::unbounded();
        let (rtx, rrx) = crossbeam_channel::unbounded();
        let tr = Gate { entered: etx, release: rrx, first: true };
        let p = Pipeline::start(PipelineConfig::default(), Box::new(FakeAsr("Hello team.")),
            Box::new(tr), writer, sink.clone());
        (root, p, sink, rx, erx, rtx)
    }

    /// Espera (com limite) um evento que satisfaça `pred`.
    fn wait_for(rx: &Receiver<PipelineEvent>, pred: impl Fn(&PipelineEvent) -> bool) -> PipelineEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(e) if pred(&e) => return e,
                Ok(_) => {}
                Err(_) => panic!("evento esperado não chegou a tempo"),
            }
        }
    }

    #[test]
    fn translated_partial_never_follows_final() {
        let (_d, p, sink, rx, entered, release) = gated();
        for c in tone(1_500).chunks(480) { p.feed(SpeakerSource::System, c); }
        // A tradução da parcial está em andamento (bloqueada) antes de a fala fechar.
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        p.feed(SpeakerSource::System, &silence(1_000));
        wait_for(&rx, |e| matches!(e, PipelineEvent::Final(l) if l.id == "s0"));
        release.send(()).unwrap();
        p.finish(10_000).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        let fin = ev.iter().position(|e| matches!(e, PipelineEvent::Final(_))).unwrap();
        assert!(!ev[fin..].iter().any(|e| matches!(e, PipelineEvent::Partial { .. })));
        assert!(ev.iter().any(|e| matches!(e, PipelineEvent::Translated(l) if l.id == "s0")));
    }

    #[test]
    fn older_partial_translation_is_not_emitted_after_newer_partial() {
        let (_d, p, sink, rx, entered, release) = gated();
        for c in tone(1_500).chunks(480) { p.feed(SpeakerSource::System, c); }
        let v1 = match wait_for(&rx, |e| matches!(e, PipelineEvent::Partial { .. })) {
            PipelineEvent::Partial { t1, .. } => t1,
            _ => unreachable!(),
        };
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        // Parcial mais nova (v2) exibida enquanto a tradução da v1 está bloqueada.
        for c in tone(2_000).chunks(480) { p.feed(SpeakerSource::System, c); }
        wait_for(&rx, |e| matches!(e, PipelineEvent::Partial { t1, .. } if *t1 > v1));
        release.send(()).unwrap();
        p.feed(SpeakerSource::System, &silence(1_000));
        p.finish(10_000).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        let t1s: Vec<u64> = ev.iter().filter_map(|e| match e {
            PipelineEvent::Partial { id, t1, .. } if id == "s0" => Some(*t1), _ => None }).collect();
        assert!(t1s.windows(2).all(|w| w[0] <= w[1]), "t1 regrediu: {t1s:?}");
    }

    fn job(source: SpeakerSource, seq: u64, is_final: bool, end_ms: u64) -> AsrJob {
        AsrJob { source, seq, start_ms: 0, end_ms, audio: vec![], is_final }
    }

    #[test]
    fn coalesce_keeps_finals_and_latest_partial_per_source() {
        use SpeakerSource::*;
        let out = coalesce(vec![
            job(System, 0, false, 1_000), job(System, 0, false, 2_000), job(System, 0, true, 2_500),
            job(System, 1, false, 3_000), job(System, 1, false, 4_000), job(Mic, 0, false, 1_000),
        ]);
        let keys: Vec<_> = out.iter().map(|j| (j.source, j.seq, j.is_final, j.end_ms)).collect();
        assert_eq!(keys, vec![(System, 0, true, 2_500), (System, 1, false, 4_000), (Mic, 0, false, 1_000)]);
    }
}
