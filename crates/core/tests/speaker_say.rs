//! Identificação de falantes com vozes do `say` (frases fictícias). Rode com:
//! bash scripts/gen-voice-fixtures.sh
//! GOW_MEET_SPEAKER_MODEL=$PWD/target/voice-models/voxceleb_ECAPA512_LM.onnx \
//! GOW_MEET_VOICES=$PWD/target/voice-fixtures \
//!   cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
use gow_meet_core::speaker::{cosine, normalize, SpeakerEmbedder, TractEmbedder};
use gow_meet_core::wav::read_wav_16k_mono;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

/// (voz, índice da frase) → áudio. Arquivos `<Voz>_<i>.wav`.
fn fixtures() -> BTreeMap<(String, usize), Vec<f32>> {
    let dir = PathBuf::from(std::env::var("GOW_MEET_VOICES").expect("defina GOW_MEET_VOICES"));
    std::fs::read_dir(&dir).unwrap().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let (voice, i) = name.strip_suffix(".wav")?.rsplit_once('_')?;
        Some(((voice.to_string(), i.parse().ok()?), read_wav_16k_mono(&e.path()).unwrap()))
    }).collect()
}

fn model() -> PathBuf {
    PathBuf::from(std::env::var("GOW_MEET_SPEAKER_MODEL").expect("defina GOW_MEET_SPEAKER_MODEL"))
}

#[test]
#[ignore]
fn embeddings_tell_say_voices_apart() {
    let mut emb = TractEmbedder::new(&model()).unwrap();
    let mut times = vec![];
    let embs: Vec<(String, Vec<f32>)> = fixtures().into_iter().map(|((voice, _), audio)| {
        let t0 = Instant::now();
        let e = emb.embed(&audio).unwrap();
        times.push(t0.elapsed().as_millis() as u64);
        assert!((e.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-4, "impressão sem norma 1");
        (voice, e)
    }).collect();
    // Deixa-um-de-fora: cada fala comparada aos centroides das vozes (sem ela mesma).
    let mut wrong = 0;
    for (i, (voice, e)) in embs.iter().enumerate() {
        let mut sums: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
        for (j, (v, x)) in embs.iter().enumerate() {
            if i == j { continue; }
            let s = sums.entry(v.as_str()).or_insert_with(|| vec![0.0; x.len()]);
            for (a, b) in s.iter_mut().zip(x) { *a += b; }
        }
        let best = sums.into_iter().map(|(v, mut c)| { normalize(&mut c); (v, cosine(&c, e)) })
            .max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0;
        if best != voice { wrong += 1; eprintln!("{voice} → {best}"); }
    }
    times.sort_unstable();
    let p90 = times[(times.len() * 9).div_ceil(10) - 1];
    eprintln!("{} falas, {wrong} erradas | impressão: mediana {} ms, p90 {p90} ms", embs.len(), times[times.len() / 2]);
    assert_eq!(wrong, 0);
    assert!(p90 <= 150, "p90 = {p90} ms");
}
