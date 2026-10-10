//! Impressões de voz e nomes nunca vão para os logs; impressões nunca vão para os
//! arquivos da sessão. Binário de teste próprio: instala um coletor de logs global.
use anyhow::Result;
use gow_meet_core::asr::Transcriber;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink, SpeakerId};
use gow_meet_core::session::{Meta, SessionWriter, SpeakerSource};
use gow_meet_core::speaker::SpeakerEmbedder;
use gow_meet_core::tracker::{SpeakerTracker, TrackerConfig};
use gow_meet_core::translate::Translator;
use gow_meet_core::voices::{VoiceLibrary, VOICES_FILE};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

struct FakeAsr;
impl Transcriber for FakeAsr {
    fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok("Hello team.".into()) }
}
struct Prefix;
impl Translator for Prefix {
    fn translate(&mut self, t: &str) -> Result<String> { Ok(format!("pt: {t}")) }
}
/// Valores fáceis de procurar nos arquivos e logs.
struct Marked;
impl SpeakerEmbedder for Marked {
    fn embed(&mut self, _a: &[f32]) -> Result<Vec<f32>> { Ok(vec![0.123_456_7, 0.765_432_1, 0.0]) }
}
struct Chan(crossbeam_channel::Sender<PipelineEvent>);
impl Sink for Chan {
    fn emit(&self, ev: PipelineEvent) { let _ = self.0.send(ev); }
}

#[test]
fn embeddings_and_names_never_reach_logs_or_session_files() {
    // Sem hora no log: um horário nunca pode conter por acaso os valores procurados.
    let logs = Capture::default();
    let c = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_ansi(false).without_time().with_max_level(tracing::Level::TRACE)
            .with_writer(move || c.clone()).finish()).unwrap();

    let lib_dir = tempfile::tempdir().unwrap();
    let lib_path = lib_dir.path().join(VOICES_FILE);
    std::fs::write(&lib_path, "{ corrompido: Fulana Fictícia 0.1234567").unwrap();
    let (library, backup) = VoiceLibrary::load(&lib_path, 1);
    assert!(backup.is_some());
    let root = tempfile::tempdir().unwrap();
    let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    let (tx, rx) = crossbeam_channel::unbounded();
    let sid = SpeakerId { embedder: Box::new(Marked),
                          tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) };
    let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr), Box::new(Prefix),
        Some(sid), SessionWriter::create(root.path(), meta).unwrap(), Arc::new(Chan(tx)));
    let speech: Vec<f32> = (0..1_500 * 16).map(|i| 0.3 * (i as f32 * 0.17).sin())
        .chain(std::iter::repeat_n(0.0, 1_000 * 16)).collect();
    p.feed(SpeakerSource::System, &speech);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())), Ok(PipelineEvent::Final(_))) {
        assert!(Instant::now() < deadline, "fala final não chegou");
    }
    p.rename_voice("v1", "Fulana Fictícia").unwrap();
    assert!(p.rename_voice("v1", "").is_err());
    p.feed(SpeakerSource::System, &speech);
    p.finish(10_000).unwrap();

    let log = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("voz nomeada"), "o teste precisa ver os logs: {log}");
    for secret in ["Fulana", "0.123", "0.765", "Hello team"] {
        assert!(!log.contains(secret), "log contém {secret:?}:\n{log}");
    }
    for entry in std::fs::read_dir(root.path().join("t")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in ["0.123", "0.765", "embedding"] {
            assert!(!text.contains(secret), "{} contém {secret:?}", path.display());
        }
    }
    // A impressão só existe no voices.json.
    assert!(std::fs::read_to_string(&lib_path).unwrap().contains("embedding"));
}
