use anyhow::Result;
use clap::Parser;
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{session_id, Meta, SessionWriter, SpeakerSource};
use gow_meet_core::translate::Ct2Translator;
use gow_meet_core::wav::read_wav_16k_mono;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Roda o pipeline sobre um WAV, em ritmo real, e imprime latências.
#[derive(Parser)]
struct Args {
    wav: PathBuf,
    #[arg(long)] whisper: PathBuf,
    #[arg(long)] opus: PathBuf,
    /// Prefixo de idioma de destino do Opus-MT (ex.: '>>pob<<').
    #[arg(long)] prefix: Option<String>,
    /// Desliga a GPU do Whisper.
    #[arg(long)] cpu: bool,
    /// Alimenta o áudio sem esperar o tempo real (latência não é medida).
    #[arg(long)] fast: bool,
    /// Repete o áudio N vezes (ex.: 24 × 25 s ≈ 10 min).
    #[arg(long, default_value_t = 1)] repeat: usize,
    #[arg(long, default_value = "gow-meet-cli-out")] out: PathBuf,
}

struct Term {
    start: Instant,
    /// Silêncio que o segmentador espera para fechar uma fala; o t1 da fala final
    /// inclui esse silêncio, então ele é descontado da latência.
    silence_close_ms: u64,
    finals: Mutex<Vec<u64>>,
    partials: Mutex<Vec<u64>>,
}

impl Sink for Term {
    fn emit(&self, ev: PipelineEvent) {
        let now = self.start.elapsed().as_millis() as u64;
        match ev {
            PipelineEvent::Partial { t1, text, translation, .. } => {
                if translation.is_none() { self.partials.lock().unwrap().push(now.saturating_sub(t1)); }
                eprintln!("    … {}", translation.unwrap_or(text));
            }
            PipelineEvent::Final(l) => println!("[{}] {}", l.speaker.label, l.text),
            PipelineEvent::Translated(l) => {
                // "Fala fechada" = fim da fala, não fim do buffer: o t1 final já inclui
                // os `silence_close_ms` de silêncio que fecharam o segmento.
                let speech_end = l.t1.saturating_sub(self.silence_close_ms);
                self.finals.lock().unwrap().push(now.saturating_sub(speech_end));
                println!("    → {}", l.translation.as_deref().unwrap_or("(sem tradução)"));
            }
            PipelineEvent::Lag { ms } => eprintln!("! atraso de {ms} ms"),
            PipelineEvent::Error(e) => eprintln!("! {e}"),
            // Parcial descartada: no CLI as parciais são linhas de log, nada a limpar.
            PipelineEvent::Dropped { .. } => {}
        }
    }
}

fn percentile(v: &[u64], p: f64) -> u64 {
    if v.is_empty() { return 0; }
    let mut s = v.to_vec();
    s.sort_unstable();
    let rank = ((p * s.len() as f64) / 100.0).ceil().max(1.0) as usize;
    s[rank - 1]
}

fn main() -> Result<()> {
    let a = Args::parse();
    let one = read_wav_16k_mono(&a.wav)?;
    let audio: Vec<f32> = std::iter::repeat(one).take(a.repeat).flatten().collect();
    let asr = WhisperTranscriber::new(&a.whisper, "en", !a.cpu)?;
    let tr = Ct2Translator::new(&a.opus, a.prefix.clone())?;
    let now = chrono::Local::now();
    let meta = Meta { v: 1, id: session_id(now), title: "CLI".into(), started: now.timestamp_millis() as u64,
        ended: None, source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
        app_version: gow_meet_core::version().into() };
    let started = meta.started;
    let writer = SessionWriter::create(&a.out, meta)?;
    let cfg = PipelineConfig::default();
    let sink = Arc::new(Term { start: Instant::now(), silence_close_ms: cfg.segmenter.silence_close_ms,
        finals: Mutex::new(vec![]), partials: Mutex::new(vec![]) });
    let p = Pipeline::start(cfg, Box::new(asr), Box::new(tr), writer, sink.clone());
    for (i, chunk) in audio.chunks(1_600).enumerate() {
        if !a.fast {
            let due = sink.start + Duration::from_millis((i as u64 + 1) * 100);
            if let Some(wait) = due.checked_duration_since(Instant::now()) { std::thread::sleep(wait); }
        }
        p.feed(SpeakerSource::System, chunk);
    }
    let audio_ms = audio.len() as u64 / 16;
    // `ended` é Unix ms: início da sessão + tempo decorrido.
    let md = p.finish(started + sink.start.elapsed().as_millis() as u64)?;
    let wall = sink.start.elapsed().as_millis() as u64;
    let f = sink.finals.lock().unwrap();
    let pa = sink.partials.lock().unwrap();
    eprintln!("\náudio: {audio_ms} ms | tempo total: {wall} ms | falas: {}", f.len());
    if !a.fast {
        eprintln!("fala fechada → tradução: p50 {} ms, p90 {} ms", percentile(&f, 50.0), percentile(&f, 90.0));
        eprintln!("parcial (transcrição): p50 {} ms, p90 {} ms", percentile(&pa, 50.0), percentile(&pa, 90.0));
    }
    eprintln!("transcrição: {}", md.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn percentile_nearest_rank() {
        let v = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        assert_eq!(super::percentile(&v, 50.0), 500);
        assert_eq!(super::percentile(&v, 90.0), 900);
        assert_eq!(super::percentile(&[], 90.0), 0);
    }
}
