//! Rode com: GOW_MEET_WHISPER_PT=$PWD/models/tmp/ggml-small-q5_1.bin cargo test -p gow-meet-core --release --test asr_whisper_pt -- --ignored --nocapture
use gow_meet_core::asr::{clean_transcript, Transcriber, WhisperTranscriber};
use gow_meet_core::lang::Lang;
use gow_meet_core::{wav::read_wav_16k_mono, wer::wer};
use std::path::{Path, PathBuf};

fn model() -> PathBuf { PathBuf::from(std::env::var("GOW_MEET_WHISPER_PT").expect("defina GOW_MEET_WHISPER_PT")) }

#[test]
#[ignore]
fn portuguese_sentences_have_low_wer() {
    let mut t = WhisperTranscriber::new(&model(), "pt", true).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pt");
    let mut total = 0.0;
    for i in 1..=5 {
        let audio = read_wav_16k_mono(&dir.join(format!("s{i}.wav"))).unwrap();
        let reference = std::fs::read_to_string(dir.join(format!("s{i}.txt"))).unwrap();
        let start = std::time::Instant::now();
        let hyp = clean_transcript(&t.transcribe(&audio).unwrap(), Lang::Pt).unwrap_or_default();
        let e = wer(&reference, &hyp);
        eprintln!("s{i}: wer={e:.2} ({:?}) | {hyp}", start.elapsed());
        total += e;
    }
    assert!(total / 5.0 < 0.20, "WER médio = {}", total / 5.0);
}

/// Imprime o que o Whisper devolve em silêncio e ruído baixo: alimenta a lista de alucinações PT.
#[test]
#[ignore]
fn silence_in_portuguese() {
    let mut t = WhisperTranscriber::new(&model(), "pt", true).unwrap();
    let noise: Vec<f32> = (0..16_000 * 3).map(|i| 0.002 * ((i * 7919 % 1000) as f32 / 1000.0 - 0.5)).collect();
    for (name, audio) in [("silêncio", vec![0.0; 16_000 * 3]), ("ruído", noise)] {
        let raw = t.transcribe(&audio).unwrap();
        eprintln!("{name}: bruto={raw:?} limpo={:?}", clean_transcript(&raw, Lang::Pt));
    }
}
