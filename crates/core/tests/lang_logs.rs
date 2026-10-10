//! Fala de um idioma sem modelo (ou sem tradutor) é descartada com um aviso no log, uma
//! vez por idioma, sem o texto da fala. Binário de teste próprio: instala um coletor de logs global.
use anyhow::Result;
use gow_meet_core::asr::Transcriber;
use gow_meet_core::lang::Lang;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{Meta, SessionWriter, SpeakerSource};
use gow_meet_core::translate::Translator;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

struct FakeAsr(&'static str);
impl Transcriber for FakeAsr {
    fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok(self.0.into()) }
}
struct Prefix;
impl Translator for Prefix {
    fn translate(&mut self, t: &str) -> Result<String> { Ok(format!("pt: {t}")) }
}
struct PanicTr;
impl Translator for PanicTr {
    fn translate(&mut self, _t: &str) -> Result<String> { panic!("falha simulada no tradutor") }
}
struct Nothing;
impl Sink for Nothing {
    fn emit(&self, _ev: PipelineEvent) {}
}

fn spoken() -> Vec<f32> {
    (0..1_500 * 16).map(|i| 0.3 * (i as f32 * 0.17).sin()).chain(std::iter::repeat_n(0.0, 1_000 * 16)).collect()
}

fn session(dir: &std::path::Path, asr: &'static str, tr: Box<dyn Translator>) -> Pipeline {
    let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    Pipeline::start(PipelineConfig::default(), Box::new(FakeAsr(asr)), tr,
                    SessionWriter::create(dir, meta).unwrap(), Arc::new(Nothing))
}

#[test]
fn speech_without_model_is_logged_once_per_language_without_text() {
    let logs = Capture::default();
    let c = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_ansi(false).without_time().with_max_level(tracing::Level::TRACE)
            .with_writer(move || c.clone()).finish()).unwrap();

    // Português nunca carregado: as falas em português são descartadas.
    let a = tempfile::tempdir().unwrap();
    let p = session(a.path(), "Hello team.", Box::new(Prefix));
    p.set_language(Lang::Pt);
    for _ in 0..3 { p.feed(SpeakerSource::System, &spoken()); }
    p.finish(10_000).unwrap();

    // Tradutor do inglês desligado por pânico: as falas seguintes saem sem tradução.
    let b = tempfile::tempdir().unwrap();
    let p = session(b.path(), "Bom dia equipe.", Box::new(PanicTr));
    for _ in 0..3 { p.feed(SpeakerSource::System, &spoken()); }
    p.finish(10_000).unwrap();

    let log = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let asr: Vec<_> = log.lines().filter(|l| l.contains("fala descartada: idioma sem modelo")).collect();
    assert_eq!(asr.len(), 1, "{log}");
    assert!(asr[0].contains("lang=\"pt\""), "{log}");
    let tr: Vec<_> = log.lines().filter(|l| l.contains("tradução descartada: idioma sem tradutor")).collect();
    assert_eq!(tr.len(), 1, "{log}");
    assert!(tr[0].contains("lang=\"en\""), "{log}");
    for secret in ["Hello", "Bom dia"] {
        assert!(!log.contains(secret), "log contém {secret:?}:\n{log}");
    }
}
