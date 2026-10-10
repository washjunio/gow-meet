//! GOW_MEET_OPUS_PT=$PWD/models/cand/<candidato> [GOW_MEET_OPUS_PT_PREFIX='>>eng<<'] cargo test -p gow-meet-core --release --test translate_ct2_pt -- --ignored --nocapture
use gow_meet_core::translate::{Ct2Translator, Translator};
use std::path::PathBuf;

#[test]
#[ignore]
fn translates_meeting_sentences_to_english() {
    let dir = PathBuf::from(std::env::var("GOW_MEET_OPUS_PT").expect("defina GOW_MEET_OPUS_PT"));
    let prefix = std::env::var("GOW_MEET_OPUS_PT_PREFIX").ok().filter(|s| !s.is_empty());
    let mut t = Ct2Translator::new(&dir, prefix).unwrap();
    let cases = [
        ("Vamos revisar o cronograma do projeto na próxima semana.", "week"),
        ("O novo painel precisa ficar pronto até sexta-feira.", "friday"),
        ("Você pode compartilhar a estimativa de orçamento com a equipe?", "budget"),
        ("Precisamos decidir quais funcionalidades entram na primeira versão.", "first"),
        ("Vou mandar um resumo depois desta reunião.", "summary"),
        ("Alguém tem alguma dúvida antes de seguirmos?", "question"),
        ("A gente pode marcar outra conversa na quinta à tarde.", "thursday"),
        ("O cliente pediu para adiar a entrega em duas semanas.", "two weeks"),
    ];
    let mut times = vec![];
    let mut misses = vec![];
    for (pt, must) in cases {
        let start = std::time::Instant::now();
        let en = t.translate(pt).unwrap();
        times.push(start.elapsed().as_millis());
        eprintln!("{pt} -> {en}  ({:?})", start.elapsed());
        if !en.to_lowercase().contains(must) { misses.push(format!("{en:?} sem {must:?}")); }
    }
    times.sort_unstable();
    eprintln!("tempo por frase: p50 {} ms, p90 {} ms", times[times.len() / 2], times[times.len() * 9 / 10]);
    assert!(misses.is_empty(), "{misses:#?}");
}
