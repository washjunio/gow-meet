use crate::asr::{clean_transcript, Transcriber};
use crate::segmenter::{RmsDetector, SegmentEvent, Segmenter, SegmenterConfig};
use crate::session::{Line, SessionWriter, Speaker, SpeakerSource, Speakers};
use crate::speaker::SpeakerEmbedder;
use crate::tracker::SpeakerTracker;
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
    /// Grupo de voz renomeado: todas as falas com `speaker.voice == voice` mostram `label`.
    Speaker { voice: String, label: String },
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

/// Identificação de quem fala no áudio do sistema (etapa 2).
pub struct SpeakerId {
    pub embedder: Box<dyn SpeakerEmbedder>,
    pub tracker: SpeakerTracker,
}

/// Aviso único quando a identificação para por erro interno (pânico no modelo de voz).
pub const SPEAKER_PANIC_MESSAGE: &str =
    "A identificação de quem fala parou por um erro interno; as falas seguem como \"Participante\".";

/// Gravação com nova tentativa: falas que falharam ficam em memória e são
/// regravadas antes da próxima.
struct Store { writer: SessionWriter, pending: Vec<Line> }

impl Store {
    /// `speakers.json`: os rótulos ficam na memória do writer mesmo se a gravação falhar;
    /// a próxima mudança e o fim da sessão tentam de novo.
    fn set_speakers(&mut self, speakers: Speakers, sink: &dyn Sink) {
        if let Err(e) = self.writer.set_speakers(speakers) {
            tracing::warn!("speakers.json não gravado: {e:#}");
            sink.emit(PipelineEvent::Error("falha ao gravar os nomes das vozes desta reunião; nova tentativa na próxima mudança".into()));
        }
    }

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

/// Rastreador compartilhado entre a thread de transcrição e `rename_voice`. Ordem de
/// locks: rastreador → store → parciais abertas.
type SharedTracker = Arc<Mutex<SpeakerTracker>>;

/// O que a thread de transcrição usa para identificar quem fala.
struct Identify {
    embedder: Box<dyn SpeakerEmbedder>,
    tracker: SharedTracker,
    /// Silêncio de fechamento no fim de cada fala final: fica fora da impressão.
    trailing_ms: u64,
    dead: bool,
}

pub struct Pipeline {
    cfg: PipelineConfig,
    segmenters: Mutex<HashMap<SpeakerSource, Segmenter>>,
    fed: Arc<Mutex<HashMap<SpeakerSource, u64>>>,
    asr_tx: Option<Sender<AsrJob>>,
    asr: Option<JoinHandle<()>>,
    tr: Option<JoinHandle<()>>,
    store: Arc<Mutex<Option<Store>>>,
    tracker: Option<SharedTracker>,
    sink: Arc<dyn Sink>,
}

fn now_ms() -> u64 { chrono::Utc::now().timestamp_millis() as u64 }

impl Pipeline {
    pub fn start(
        cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>,
        writer: SessionWriter, sink: Arc<dyn Sink>,
    ) -> Self {
        Self::start_with_speakers(cfg, asr, translator, None, writer, sink)
    }

    /// Como `start`, identificando quem fala no áudio do sistema quando `speakers` vem.
    pub fn start_with_speakers(
        cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>,
        speakers: Option<SpeakerId>, writer: SessionWriter, sink: Arc<dyn Sink>,
    ) -> Self {
        let (asr_tx, asr_rx) = unbounded::<AsrJob>();
        let (tr_tx, tr_rx) = unbounded::<TJob>();
        let store = Arc::new(Mutex::new(Some(Store { writer, pending: vec![] })));
        let fed = Arc::new(Mutex::new(HashMap::new()));
        let open: OpenPartials = Arc::new(Mutex::new(HashMap::new()));
        let (identify, tracker) = match speakers {
            Some(SpeakerId { embedder, tracker }) => {
                let shared: SharedTracker = Arc::new(Mutex::new(tracker));
                let id = Identify { embedder, tracker: shared.clone(),
                                    trailing_ms: cfg.segmenter.silence_close_ms, dead: false };
                (Some(id), Some(shared))
            }
            None => (None, None),
        };
        let asr_h = {
            let (store, sink, fed, warn, open) =
                (store.clone(), sink.clone(), fed.clone(), cfg.lag_warn_ms, open.clone());
            std::thread::Builder::new().name("asr".into())
                .spawn(move || asr_loop(asr_rx, asr, identify, tr_tx, store, sink, fed, warn, open)).expect("thread asr")
        };
        let tr_h = {
            let (store, sink, every) = (store.clone(), sink.clone(), Duration::from_millis(cfg.partial_translate_every_ms));
            std::thread::Builder::new().name("translate".into())
                .spawn(move || translate_loop(tr_rx, translator, store, sink, every, open)).expect("thread translate")
        };
        Self { cfg, segmenters: Mutex::new(HashMap::new()), fed, asr_tx: Some(asr_tx),
               asr: Some(asr_h), tr: Some(tr_h), store, tracker, sink }
    }

    /// Dá nome ao grupo `voice` durante a reunião: biblioteca de vozes, `speakers.json` e
    /// evento `Speaker`. Erro (sem efeito) para identificação desligada, voz inexistente
    /// ou nome vazio.
    pub fn rename_voice(&self, voice: &str, name: &str) -> Result<String> {
        let tracker = self.tracker.as_ref().context("a identificação de vozes está desligada nesta reunião")?;
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let a = t.rename(voice, name, now_ms())?;
        if let Err(e) = t.save_library() {
            tracing::warn!("voices.json não gravado: {e:#}");
            self.sink.emit(PipelineEvent::Error(
                "falha ao gravar as vozes guardadas; o nome vale nesta reunião e será gravado de novo ao encerrar".into()));
        }
        if let Some(s) = self.store.lock().unwrap().as_mut() { s.set_speakers(t.speakers(), self.sink.as_ref()); }
        // Emitido com o rastreador travado: nenhuma fala final do grupo com o rótulo
        // antigo sai depois deste evento.
        self.sink.emit(PipelineEvent::Speaker { voice: a.voice, label: a.label.clone() });
        Ok(a.label)
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
        if let Some(tracker) = self.tracker.take() {
            let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
            t.finish(now_ms());
            // Só no log: a reunião acabou, não há tela para avisar; a mensagem não leva nomes.
            if let Err(e) = t.save_library() { tracing::warn!("voices.json não gravado ao encerrar: {e:#}"); }
            let speakers = t.speakers();
            if !speakers.is_empty() {
                // Falha aqui não perde os nomes: o Markdown usa os rótulos em memória.
                if let Err(e) = writer.set_speakers(speakers) { tracing::warn!("speakers.json não gravado: {e:#}"); }
            }
        }
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

impl Identify {
    /// Impressão de voz de `audio` (16 kHz) se a fala tem ao menos `min_ms`; `None` se é
    /// curta, se o cálculo falhou ou se a identificação morreu por pânico. Só falhas de
    /// falas finais (`is_final`) são contadas; parciais só espiam.
    fn embed(&mut self, audio: &[f32], speech_ms: u64, is_final: bool, sink: &dyn Sink) -> Option<Vec<f32>> {
        if self.dead { return None; }
        let min_ms = self.tracker.lock().unwrap_or_else(|e| e.into_inner()).min_speech_ms();
        if speech_ms < min_ms { return None; }
        match guarded(|| self.embedder.embed(audio)) {
            Some(Ok(e)) => Some(e),
            Some(Err(e)) => {
                tracing::debug!("impressão de voz falhou: {e}");
                if is_final { self.tracker.lock().unwrap_or_else(|e| e.into_inner()).note_embed_failure(); }
                None
            }
            None => {
                self.dead = true;
                tracing::error!("pânico no modelo de voz; identificação desligada nesta sessão");
                sink.emit(PipelineEvent::Error(SPEAKER_PANIC_MESSAGE.into()));
                None
            }
        }
    }
}

fn asr_loop(
    rx: Receiver<AsrJob>, mut asr: Box<dyn Transcriber>, mut identify: Option<Identify>, tr_tx: Sender<TJob>,
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
            let mut speaker = speaker_for(job.source);
            // Só o áudio do sistema passa pela identificação; o microfone é sempre "Você".
            let ident = identify.as_mut().filter(|_| job.source == SpeakerSource::System);
            if job.is_final {
                let mut line = Line { id, t0: job.start_ms, t1: job.end_ms, speaker, text,
                                      translation: None, translation_error: false };
                // O rastreador fica travado até o evento sair: um `rename_voice` concorrente
                // é aplicado antes (rótulo novo aqui) ou depois (evento `Speaker` depois).
                let mut tracked = None;
                if let Some(ident) = ident {
                    let trailing = (ident.trailing_ms * 16) as usize;
                    let audio = &job.audio[..job.audio.len().saturating_sub(trailing)];
                    let speech_ms = (job.end_ms - job.start_ms).saturating_sub(ident.trailing_ms);
                    let emb = ident.embed(audio, speech_ms, true, sink.as_ref());
                    let mut t = ident.tracker.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(a) = t.assign(emb.as_deref(), speech_ms) {
                        line.speaker = Speaker::tracked(&a.voice, &a.label);
                    }
                    tracked = Some(t);
                }
                if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                {
                    let mut o = open.lock().unwrap();
                    o.remove(&line.id);
                    sink.emit(PipelineEvent::Final(line.clone()));
                }
                drop(tracked);
                let _ = tr_tx.send(TJob::Final(line));
            } else {
                // Parcial: rótulo provável, sem mudar grupos nem contar falhas; a fala final confirma.
                if let Some(ident) = ident {
                    let speech_ms = job.end_ms - job.start_ms;
                    if let Some(emb) = ident.embed(&job.audio, speech_ms, false, sink.as_ref()) {
                        let t = ident.tracker.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(label) = t.peek(&emb) { speaker.label = label; }
                    }
                }
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

    // ---------- Identificação de quem fala (etapa 2) ----------

    use crate::tracker::{SpeakerTracker, TrackerConfig};
    use crate::voices::{VoiceLibrary, VOICES_FILE};

    /// Fala com amplitude própria: o embedder falso distingue "pessoas" pela energia.
    fn voice_tone(amp: f32, ms: usize) -> Vec<f32> { (0..ms * MS).map(|i| amp * (i as f32 * 0.17).sin()).collect() }
    const QUIET: f32 = 0.1;
    const LOUD: f32 = 0.4;

    /// Impressão falsa: [1,0,0] para fala baixa, [0,1,0] para fala alta.
    struct EnergyEmbedder;
    impl SpeakerEmbedder for EnergyEmbedder {
        fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
            let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len().max(1) as f32).sqrt();
            Ok(if rms < 0.15 { vec![1.0, 0.0, 0.0] } else { vec![0.0, 1.0, 0.0] })
        }
    }
    struct PanicEmbedder;
    impl SpeakerEmbedder for PanicEmbedder {
        fn embed(&mut self, _a: &[f32]) -> Result<Vec<f32>> { panic!("falha simulada no modelo de voz") }
    }

    fn speaker_id(lib_dir: &std::path::Path, embedder: Box<dyn SpeakerEmbedder>) -> SpeakerId {
        let library = VoiceLibrary::empty(&lib_dir.join(VOICES_FILE));
        SpeakerId { embedder, tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) }
    }

    fn start_tracked(lib_dir: &std::path::Path, embedder: Box<dyn SpeakerEmbedder>)
        -> (tempfile::TempDir, Pipeline, Arc<ChanSink>, Receiver<PipelineEvent>) {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = Arc::new(ChanSink(Mutex::new(vec![]), tx));
        let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr("Hello team.")),
            Box::new(Prefix), Some(speaker_id(lib_dir, embedder)), writer, sink.clone());
        (root, p, sink, rx)
    }

    fn utterance(p: &Pipeline, source: SpeakerSource, amp: f32) {
        p.feed(source, &[voice_tone(amp, 1_500), silence(1_000)].concat());
    }

    fn finals(ev: &[PipelineEvent]) -> Vec<Line> {
        ev.iter().filter_map(|e| match e { PipelineEvent::Final(l) => Some(l.clone()), _ => None }).collect()
    }

    #[test]
    fn system_final_already_has_voice_and_label_and_mic_stays_voce() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, sink, _rx) = start_tracked(lib.path(), Box::new(EnergyEmbedder));
        utterance(&p, SpeakerSource::System, QUIET);
        utterance(&p, SpeakerSource::Mic, LOUD);
        utterance(&p, SpeakerSource::System, LOUD);
        utterance(&p, SpeakerSource::System, QUIET);
        p.finish(10_000).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        let got: Vec<(String, String, Option<String>)> = finals(&ev).into_iter()
            .map(|l| (l.id, l.speaker.label, l.speaker.voice)).collect();
        assert_eq!(got, vec![
            ("s0".into(), "Participante 1".into(), Some("v1".into())),
            ("m0".into(), "Você".into(), None),
            ("s1".into(), "Participante 2".into(), Some("v2".into())),
            ("s2".into(), "Participante 1".into(), Some("v1".into())),
        ]);
        // Nenhuma fala final do sistema sai como "Participante" genérico para depois mudar.
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(l) if l.speaker.label == "Participante")));
    }

    #[test]
    fn rename_emits_speaker_and_names_earlier_and_later_lines() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, sink, rx) = start_tracked(lib.path(), Box::new(EnergyEmbedder));
        utterance(&p, SpeakerSource::System, QUIET);
        wait_for(&rx, |e| matches!(e, PipelineEvent::Final(l) if l.id == "s0"));
        assert_eq!(p.rename_voice("v1", " Ana ").unwrap(), "Ana");
        assert!(p.rename_voice("v9", "Bruno").is_err());
        assert!(p.rename_voice("v1", "   ").is_err());
        utterance(&p, SpeakerSource::System, QUIET);
        let md = std::fs::read_to_string(p.finish(10_000).unwrap()).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        let sp: Vec<_> = ev.iter().filter(|e| matches!(e, PipelineEvent::Speaker { .. })).cloned().collect();
        assert_eq!(sp, vec![PipelineEvent::Speaker { voice: "v1".into(), label: "Ana".into() }]);
        let f = finals(&ev);
        assert_eq!(f[1].speaker, Speaker::tracked("v1", "Ana"));
        // Markdown: a fala anterior à renomeação também sai com o nome.
        assert_eq!(md.matches("] Ana:**").count(), 2, "{md}");
        let dir = _d.path().join("t");
        let speakers = crate::session::read_speakers(&dir);
        assert_eq!(speakers["v1"].label, "Ana");
        assert!(speakers["v1"].voice_id.is_some());
        let (library, _) = VoiceLibrary::load(&lib.path().join(VOICES_FILE), 0);
        let s = library.summaries();
        assert_eq!((s.len(), s[0].name.as_str(), s[0].count), (1, "Ana", 2));
    }

    #[test]
    fn named_voice_is_recognized_in_the_next_meeting() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, _s, rx) = start_tracked(lib.path(), Box::new(EnergyEmbedder));
        utterance(&p, SpeakerSource::System, LOUD);
        wait_for(&rx, |e| matches!(e, PipelineEvent::Final(_)));
        p.rename_voice("v1", "Bruno").unwrap();
        p.finish(10_000).unwrap();
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t2".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
        let (library, _) = VoiceLibrary::load(&lib.path().join(VOICES_FILE), 0);
        let sid = SpeakerId { embedder: Box::new(EnergyEmbedder),
                              tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) };
        let sink = Arc::new(Collect::default());
        let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr("Hello again.")),
            Box::new(Prefix), Some(sid), SessionWriter::create(root.path(), meta).unwrap(), sink.clone());
        utterance(&p, SpeakerSource::System, QUIET);
        utterance(&p, SpeakerSource::System, LOUD);
        p.finish(10_000).unwrap();
        let labels: Vec<String> = finals(&sink.0.lock().unwrap()).into_iter().map(|l| l.speaker.label).collect();
        assert_eq!(labels, vec!["Participante 1".to_string(), "Bruno".to_string()]);
    }

    #[test]
    fn rename_without_identification_is_an_error() {
        let (_d, _ev, _lines) = run("Hello team.", Box::new(Prefix), |p| {
            assert!(p.rename_voice("v1", "Ana").is_err());
        });
    }

    #[test]
    fn embedder_panic_emits_one_error_and_lines_stay_participante() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, sink, _rx) = start_tracked(lib.path(), Box::new(PanicEmbedder));
        utterance(&p, SpeakerSource::System, QUIET);
        utterance(&p, SpeakerSource::System, LOUD);
        p.finish(10_000).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        assert_eq!(errors(&ev), vec![SPEAKER_PANIC_MESSAGE.to_string()]);
        let f = finals(&ev);
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|l| l.speaker == Speaker::system()));
    }

    /// Original corrompido que não pôde ser preservado: `save` recusa. O nome vale na
    /// reunião, a UI recebe um aviso na renomeação e o encerramento não falha.
    #[cfg(unix)]
    #[test]
    fn library_save_refusal_is_not_fatal_and_keeps_the_original() {
        use std::os::unix::fs::PermissionsExt;
        let lib = tempfile::tempdir().unwrap();
        let path = lib.path().join(VOICES_FILE);
        std::fs::write(&path, "{ corrompido").unwrap();
        std::fs::set_permissions(lib.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let (library, backup) = VoiceLibrary::load(&path, 1);
        assert!(backup.is_none());
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = Arc::new(ChanSink(Mutex::new(vec![]), tx));
        let sid = SpeakerId { embedder: Box::new(EnergyEmbedder),
                              tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) };
        let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr("Hello team.")),
            Box::new(Prefix), Some(sid), SessionWriter::create(root.path(), meta).unwrap(), sink.clone());
        utterance(&p, SpeakerSource::System, QUIET);
        wait_for(&rx, |e| matches!(e, PipelineEvent::Final(_)));
        assert_eq!(p.rename_voice("v1", "Ana").unwrap(), "Ana");
        let md = std::fs::read_to_string(p.finish(10_000).unwrap()).unwrap();
        std::fs::set_permissions(lib.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(md.contains("] Ana:**"), "{md}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ corrompido");
        let ev = sink.0.lock().unwrap().clone();
        assert_eq!(errors(&ev).len(), 1, "{ev:?}");
        assert!(!errors(&ev)[0].contains("Ana"));
    }

    /// Impressão que sempre falha (sem pânico).
    struct FailEmbedder;
    impl SpeakerEmbedder for FailEmbedder {
        fn embed(&mut self, _a: &[f32]) -> Result<Vec<f32>> { anyhow::bail!("falha simulada") }
    }

    #[test]
    fn partial_embed_failure_is_not_counted_only_final_is() {
        let lib = tempfile::tempdir().unwrap();
        let SpeakerId { embedder, tracker } = speaker_id(lib.path(), Box::new(FailEmbedder));
        let shared: SharedTracker = Arc::new(Mutex::new(tracker));
        let mut ident = Identify { embedder, tracker: shared.clone(), trailing_ms: 0, dead: false };
        let sink = Collect::default();
        let audio = voice_tone(QUIET, 1_500);
        assert!(ident.embed(&audio, 1_500, false, &sink).is_none());
        assert_eq!(shared.lock().unwrap().embed_failures(), 0, "parcial não conta falha");
        assert!(ident.embed(&audio, 1_500, true, &sink).is_none());
        assert_eq!(shared.lock().unwrap().embed_failures(), 1);
        assert!(sink.0.lock().unwrap().is_empty());
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
