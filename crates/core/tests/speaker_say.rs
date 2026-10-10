//! Identificação de falantes com vozes do `say` (frases fictícias). Rode com:
//! bash scripts/gen-voice-fixtures.sh
//! GOW_MEET_SPEAKER_MODEL=$PWD/target/voice-models/voxceleb_ECAPA512_LM.onnx \
//! GOW_MEET_VOICES=$PWD/target/voice-fixtures \
//!   cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
use anyhow::Result;
use gow_meet_core::asr::Transcriber;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink, SpeakerId};
use gow_meet_core::session::{Meta, SessionWriter, SpeakerSource};
use gow_meet_core::speaker::{cosine, normalize, SpeakerEmbedder, TractEmbedder};
use gow_meet_core::tracker::{SpeakerTracker, TrackerConfig};
use gow_meet_core::translate::Translator;
use gow_meet_core::voices::{VoiceLibrary, VOICES_FILE};
use gow_meet_core::wav::read_wav_16k_mono;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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

/// Ordem de reunião: as vozes se alternam a cada frase.
fn meeting(f: &BTreeMap<(String, usize), Vec<f32>>, sentences: std::ops::Range<usize>) -> Vec<(String, &Vec<f32>)> {
    let voices: BTreeSet<&String> = f.keys().map(|(v, _)| v).collect();
    sentences.flat_map(|i| voices.iter().map(move |v| ((*v).clone(), i)).collect::<Vec<_>>())
        .filter_map(|(v, i)| f.get(&(v.clone(), i)).map(|a| (v, a)))
        .collect()
}

#[test]
#[ignore]
fn each_say_voice_gets_its_own_group_and_is_recognized_next_time() {
    let mut emb = TractEmbedder::new(&model()).unwrap();
    let f = fixtures();
    let lib_dir = tempfile::tempdir().unwrap();
    let lib_path = lib_dir.path().join(VOICES_FILE);

    // Reunião 1: cada voz num grupo próprio, e cada grupo com uma voz só.
    let mut t = SpeakerTracker::new(TrackerConfig::default(), "teste", VoiceLibrary::empty(&lib_path));
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (voice, audio) in meeting(&f, 0..4) {
        let e = emb.embed(audio).unwrap();
        let a = t.assign(Some(&e), audio.len() as u64 / 16).unwrap();
        groups.entry(a.voice).or_default().insert(voice);
    }
    eprintln!("reunião 1: {groups:?}");
    let n_voices = f.keys().map(|(v, _)| v).collect::<BTreeSet<_>>().len();
    assert_eq!(groups.len(), n_voices, "grupos ≠ vozes: {groups:?}");
    assert!(groups.values().all(|s| s.len() == 1), "grupo com mais de uma voz: {groups:?}");

    // Nomeia cada grupo com o nome da voz do `say` (fictício) e encerra.
    for (g, voices) in &groups { t.rename(g, voices.iter().next().unwrap(), 1).unwrap(); }
    t.finish(2);
    t.save_library().unwrap();

    // Reunião 2, outras frases: cada voz reconhecida pelo nome.
    let (lib, corrupt) = VoiceLibrary::load(&lib_path, 3);
    assert!(corrupt.is_none());
    let mut t = SpeakerTracker::new(TrackerConfig::default(), "teste", lib);
    let mut wrong = 0;
    let all = meeting(&f, 4..8);
    for (voice, audio) in &all {
        let e = emb.embed(audio).unwrap();
        let a = t.assign(Some(&e), audio.len() as u64 / 16).unwrap();
        if &a.label != voice { wrong += 1; eprintln!("{voice} → {}", a.label); }
    }
    eprintln!("reunião 2: {wrong}/{} erradas", all.len());
    assert_eq!(wrong, 0);
}

struct FixedAsr;
impl Transcriber for FixedAsr {
    fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok("Sentence.".into()) }
}
struct Echo;
impl Translator for Echo {
    fn translate(&mut self, t: &str) -> Result<String> { Ok(t.into()) }
}
#[derive(Default)]
struct Finals(Mutex<Vec<(String, Option<String>)>>);
impl Sink for Finals {
    fn emit(&self, ev: PipelineEvent) {
        if let PipelineEvent::Final(l) = ev { self.0.lock().unwrap().push((l.speaker.label, l.speaker.voice)); }
    }
}

/// Uma reunião pelo pipeline: falas alternadas com 1,5 s de silêncio, em pedaços de 100 ms.
/// Com `names`, dá a cada grupo o nome da voz que mais apareceu nele antes de encerrar.
fn run_meeting(model: &Path, lib_path: &Path, order: &[(String, &Vec<f32>)], names: bool) -> Vec<(String, Option<String>)> {
    let (library, _) = VoiceLibrary::load(lib_path, 1);
    let sid = SpeakerId { embedder: Box::new(TractEmbedder::new(model).unwrap()),
                          tracker: SpeakerTracker::new(TrackerConfig::default(), "teste", library) };
    let root = tempfile::tempdir().unwrap();
    let meta = Meta { v: 1, id: "say".into(), title: "say".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    let sink = Arc::new(Finals::default());
    let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FixedAsr), Box::new(Echo),
        Some(sid), SessionWriter::create(root.path(), meta).unwrap(), sink.clone());
    let gap = vec![0.0f32; 1_500 * 16];
    for (_, audio) in order {
        for c in audio.chunks(1_600).chain(gap.chunks(1_600)) { p.feed(SpeakerSource::System, c); }
    }
    if names {
        let deadline = Instant::now() + std::time::Duration::from_secs(60);
        while sink.0.lock().unwrap().len() < order.len() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let got = sink.0.lock().unwrap().clone();
        let mut by_group: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for ((voice, _), (_, g)) in order.iter().zip(&got) {
            *by_group.entry(g.clone().unwrap_or_default()).or_default().entry(voice.clone()).or_default() += 1;
        }
        for (g, counts) in by_group {
            let name = counts.into_iter().max_by_key(|(_, n)| *n).unwrap().0;
            p.rename_voice(&g, &name).unwrap();
        }
    }
    p.finish(1).unwrap();
    let got = sink.0.lock().unwrap().clone();
    got
}

#[test]
#[ignore]
fn say_meeting_through_the_pipeline() {
    let model = model();
    let f = fixtures();
    let lib_dir = tempfile::tempdir().unwrap();
    let lib_path = lib_dir.path().join(VOICES_FILE);

    let first = meeting(&f, 0..4);
    let got = run_meeting(&model, &lib_path, &first, true);
    assert_eq!(got.len(), first.len(), "falas fechadas ≠ falas geradas");
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for ((voice, _), (_, g)) in first.iter().zip(&got) { groups.entry(g.clone().unwrap()).or_default().insert(voice.clone()); }
    eprintln!("pipeline, reunião 1: {groups:?}");
    let n_voices = f.keys().map(|(v, _)| v).collect::<BTreeSet<_>>().len();
    assert_eq!(groups.len(), n_voices);
    assert!(groups.values().all(|s| s.len() == 1));

    let second = meeting(&f, 4..8);
    let got = run_meeting(&model, &lib_path, &second, false);
    let wrong = second.iter().zip(&got).filter(|((voice, _), (label, _))| voice != label).count();
    eprintln!("pipeline, reunião 2: {wrong}/{} erradas", second.len());
    assert_eq!(wrong, 0);
}
