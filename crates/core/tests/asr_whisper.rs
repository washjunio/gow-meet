//! Rode com: GOW_MEET_WHISPER=$PWD/models/tmp/ggml-small.en-q5_1.bin cargo test -p gow-meet-core --release --test asr_whisper -- --ignored
use gow_meet_core::asr::{clean_transcript, Transcriber, WhisperTranscriber};
use gow_meet_core::{wav::read_wav_16k_mono, wer::wer};
use std::path::{Path, PathBuf};

#[test]
#[ignore]
fn synthetic_sentences_have_low_wer() {
    let model = PathBuf::from(std::env::var("GOW_MEET_WHISPER").expect("defina GOW_MEET_WHISPER"));
    let mut t = WhisperTranscriber::new(&model, "en", true).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut total = 0.0;
    for i in 1..=5 {
        let audio = read_wav_16k_mono(&dir.join(format!("s{i}.wav"))).unwrap();
        let reference = std::fs::read_to_string(dir.join(format!("s{i}.txt"))).unwrap();
        let hyp = clean_transcript(&t.transcribe(&audio).unwrap()).unwrap_or_default();
        let e = wer(&reference, &hyp);
        eprintln!("s{i}: wer={e:.2} | {hyp}");
        total += e;
    }
    assert!(total / 5.0 < 0.15, "WER médio = {}", total / 5.0);
}

#[test]
#[ignore]
fn silence_yields_no_line() {
    let model = PathBuf::from(std::env::var("GOW_MEET_WHISPER").expect("defina GOW_MEET_WHISPER"));
    let mut t = WhisperTranscriber::new(&model, "en", true).unwrap();
    let raw = t.transcribe(&vec![0.0; 16_000 * 3]).unwrap();
    assert_eq!(clean_transcript(&raw), None, "bruto: {raw:?}");
}

#[test]
#[ignore]
fn short_audio_still_transcribes() {
    let model = PathBuf::from(std::env::var("GOW_MEET_WHISPER").expect("defina GOW_MEET_WHISPER"));
    let mut t = WhisperTranscriber::new(&model, "en", true).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let audio = read_wav_16k_mono(&dir.join("s4.wav")).unwrap();
    // 0,8 s: abaixo do mínimo de 1 s do whisper.cpp.
    let short = &audio[..12_800.min(audio.len())];
    let raw = t.transcribe(short).unwrap();
    eprintln!("short: {raw:?}");
    assert!(clean_transcript(&raw).is_some(), "bruto: {raw:?}");
}
