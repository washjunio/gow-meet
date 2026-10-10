//! Troca de idioma com os modelos reais (frases fictícias do `say`). Rode com:
//! GOW_MEET_WHISPER=… GOW_MEET_OPUS=… GOW_MEET_OPUS_PREFIX='>>pob<<' \
//! GOW_MEET_WHISPER_PT=… GOW_MEET_OPUS_PT=… [GOW_MEET_OPUS_PT_PREFIX=…] \
//! cargo test -p gow-meet-core --release --test lang_switch -- --ignored --nocapture
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::lang::Lang;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{replay, Meta, SessionWriter, SpeakerSource};
use gow_meet_core::translate::Ct2Translator;
use gow_meet_core::wav::read_wav_16k_mono;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Quiet;
impl Sink for Quiet { fn emit(&self, _ev: PipelineEvent) {} }

fn env(k: &str) -> PathBuf { PathBuf::from(std::env::var(k).unwrap_or_else(|_| panic!("defina {k}"))) }
fn prefix(k: &str) -> Option<String> { std::env::var(k).ok().filter(|s| !s.is_empty()) }

#[test]
#[ignore]
fn english_then_portuguese_in_one_meeting() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let root = tempfile::tempdir().unwrap();
    let meta = Meta { v: 1, id: "troca".into(), title: "Teste".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    let writer = SessionWriter::create(root.path(), meta).unwrap();
    let p = Pipeline::start(PipelineConfig::default(),
        Box::new(WhisperTranscriber::new(&env("GOW_MEET_WHISPER"), "en", true).unwrap()),
        Box::new(Ct2Translator::new(&env("GOW_MEET_OPUS"), prefix("GOW_MEET_OPUS_PREFIX")).unwrap()),
        writer, Arc::new(Quiet));
    let silence = vec![0.0f32; 16_000];
    p.feed(SpeakerSource::System, &read_wav_16k_mono(&fx.join("s1.wav")).unwrap());
    p.feed(SpeakerSource::System, &silence);
    p.add_language(Lang::Pt,
        Box::new(WhisperTranscriber::new(&env("GOW_MEET_WHISPER_PT"), "pt", true).unwrap()),
        Box::new(Ct2Translator::new(&env("GOW_MEET_OPUS_PT"), prefix("GOW_MEET_OPUS_PT_PREFIX")).unwrap()));
    assert!(p.set_language(Lang::Pt));
    p.feed(SpeakerSource::System, &read_wav_16k_mono(&fx.join("pt/s1.wav")).unwrap());
    p.feed(SpeakerSource::System, &silence);
    let md = p.finish(60_000).unwrap();
    let lines = replay(&md.parent().unwrap().join("lines.jsonl")).unwrap();
    for l in &lines { eprintln!("{} {:?} | {} -> {:?}", l.id, l.lang, l.text, l.translation); }
    assert_eq!(lines.iter().map(|l| l.lang).collect::<Vec<_>>(), vec![Lang::En, Lang::Pt]);
    assert!(lines[0].translation.as_deref().unwrap_or("").to_lowercase().contains("semana"));
    assert!(lines[1].translation.as_deref().unwrap_or("").to_lowercase().contains("week"));
    assert!(std::fs::read_to_string(&md).unwrap().contains("*— agora em português —*"));
}
