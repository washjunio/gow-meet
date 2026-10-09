//! GOW_MEET_OPUS=$PWD/models/opus-mt-en-pt-ct2 GOW_MEET_OPUS_PREFIX='>>pob<<' cargo test -p gow-meet-core --release --test translate_ct2 -- --ignored --nocapture
use gow_meet_core::translate::{Ct2Translator, Translator};
use std::path::PathBuf;

#[test]
#[ignore]
fn translates_synthetic_sentences_to_portuguese() {
    let dir = PathBuf::from(std::env::var("GOW_MEET_OPUS").expect("defina GOW_MEET_OPUS"));
    let prefix = std::env::var("GOW_MEET_OPUS_PREFIX").ok().filter(|s| !s.is_empty());
    let mut t = Ct2Translator::new(&dir, prefix).unwrap();
    let cases = [
        ("Let's review the project timeline for next week.", "semana"),
        ("The new dashboard should be ready by Friday.", "sexta"),
        ("I will send a summary after this call.", "resumo"),
    ];
    for (en, must) in cases {
        let start = std::time::Instant::now();
        let pt = t.translate(en).unwrap();
        eprintln!("{en} -> {pt}  ({:?})", start.elapsed());
        assert!(pt.to_lowercase().contains(must), "{pt:?} não contém {must:?}");
    }
}
