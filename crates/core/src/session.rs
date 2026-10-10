use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeakerSource { Mic, System }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Speaker {
    pub source: SpeakerSource,
    pub label: String,
    /// Grupo de voz na reunião (`"v1"`, `"v2"`…). Só em falas do sistema identificadas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

impl Speaker {
    pub fn mic() -> Self { Self { source: SpeakerSource::Mic, label: "Você".into(), voice: None } }
    pub fn system() -> Self { Self { source: SpeakerSource::System, label: "Participante".into(), voice: None } }
    /// Fala do sistema atribuída ao grupo `voice`.
    pub fn tracked(voice: &str, label: &str) -> Self {
        Self { source: SpeakerSource::System, label: label.into(), voice: Some(voice.into()) }
    }
}

/// Entrada de `speakers.json`: rótulo atual do grupo e a voz guardada a que ele aponta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerEntry {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
}

/// `speakers.json`: grupo (`"v1"`) → rótulo. Sessões da etapa 1 não têm o arquivo.
pub type Speakers = BTreeMap<String, SpeakerEntry>;

const SPEAKERS_FILE: &str = "speakers.json";

pub fn write_speakers(dir: &Path, speakers: &Speakers) -> Result<()> {
    write_atomic(&dir.join(SPEAKERS_FILE), &serde_json::to_vec_pretty(speakers)?)
}

/// Ausente: vazio (sessão antiga). Inválido: vazio, com aviso sem conteúdo; as falas
/// usam então o rótulo gravado em cada linha.
pub fn read_speakers(dir: &Path) -> Speakers {
    let Ok(bytes) = fs::read(dir.join(SPEAKERS_FILE)) else { return Speakers::new() };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        tracing::warn!(line = e.line(), column = e.column(), "speakers.json inválido; usando os rótulos das falas");
        Speakers::new()
    })
}

/// Rótulo da fala: o de `speakers.json` para o grupo, se houver; senão o da própria linha.
pub fn label_for<'a>(line: &'a Line, speakers: &'a Speakers) -> &'a str {
    line.speaker.voice.as_ref().and_then(|v| speakers.get(v)).map_or(line.speaker.label.as_str(), |e| e.label.as_str())
}

fn is_false(b: &bool) -> bool { !*b }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub id: String,
    pub t0: u64,
    pub t1: u64,
    pub speaker: Speaker,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub translation_error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub v: u32,
    pub id: String,
    pub title: String,
    pub started: u64,
    pub ended: Option<u64>,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    pub app_version: String,
}

pub fn session_id(dt: chrono::DateTime<chrono::Local>) -> String {
    dt.format("%Y-%m-%d_%H%M%S").to_string()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = File::create(&tmp).with_context(|| format!("criar {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("renomear para {}", path.display()))?;
    Ok(())
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<()> {
    write_atomic(&dir.join("meta.json"), &serde_json::to_vec_pretty(meta)?)
}

pub struct SessionWriter { dir: PathBuf, meta: Meta, lines: File, speakers: Speakers }

impl SessionWriter {
    pub fn create(root: &Path, meta: Meta) -> Result<Self> {
        let dir = root.join(&meta.id);
        fs::create_dir_all(&dir).with_context(|| format!("criar {}", dir.display()))?;
        write_meta(&dir, &meta)?;
        let lines = OpenOptions::new().create(true).append(true).open(dir.join("lines.jsonl"))?;
        Ok(Self { dir, meta, lines, speakers: Speakers::new() })
    }

    pub fn dir(&self) -> &Path { &self.dir }

    pub fn append(&mut self, line: &Line) -> Result<()> {
        let mut buf = serde_json::to_vec(line)?;
        buf.push(b'\n');
        self.lines.write_all(&buf)?;
        self.lines.flush()?;
        Ok(())
    }

    /// Guarda os rótulos em memória e grava `speakers.json`. Se a gravação falhar, a
    /// memória já vale para o Markdown e a próxima chamada (ou `finish`) tenta de novo.
    pub fn set_speakers(&mut self, speakers: Speakers) -> Result<()> {
        self.speakers = speakers;
        write_speakers(&self.dir, &self.speakers)
    }

    pub fn finish(mut self, ended_ms: u64) -> Result<PathBuf> {
        self.lines.sync_all()?;
        self.meta.ended = Some(ended_ms);
        if !self.speakers.is_empty() {
            if let Err(e) = write_speakers(&self.dir, &self.speakers) {
                tracing::warn!("speakers.json não gravado ao encerrar: {e:#}");
            }
        }
        close_session(&self.dir, &self.meta, &self.speakers)
    }
}

fn close_session(dir: &Path, meta: &Meta, speakers: &Speakers) -> Result<PathBuf> {
    write_meta(dir, meta)?;
    let lines = replay(&dir.join("lines.jsonl"))?;
    let md = dir.join("transcript.md");
    write_atomic(&md, render_markdown(meta, &lines, speakers).as_bytes())?;
    Ok(md)
}

/// Lê o `lines.jsonl`; o último registro de cada `id` prevalece, na ordem da
/// primeira aparição. Linhas inválidas (ex.: truncadas por queda) são ignoradas.
pub fn replay(lines_path: &Path) -> Result<Vec<Line>> {
    let file = match File::open(lines_path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut order: Vec<String> = vec![];
    let mut latest: HashMap<String, Line> = HashMap::new();
    let mut reader = BufReader::new(file);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if reader.read_until(b'\n', &mut raw)? == 0 { break; }
        // Bytes, não `lines()`: uma queda pode cortar um caractere UTF-8 ao meio.
        let Ok(line) = serde_json::from_slice::<Line>(&raw) else {
            tracing::warn!("registro inválido ignorado em lines.jsonl");
            continue;
        };
        if !latest.contains_key(&line.id) { order.push(line.id.clone()); }
        latest.insert(line.id.clone(), line);
    }
    Ok(order.into_iter().filter_map(|id| latest.remove(&id)).collect())
}

fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

pub fn render_markdown(meta: &Meta, lines: &[Line], speakers: &Speakers) -> String {
    use chrono::TimeZone;
    let started = chrono::Local.timestamp_millis_opt(meta.started as i64).single()
        .map(|d| d.format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default();
    let mut out = format!(
        "# {}\n\n- Início: {}\n- Idiomas: {} → {}\n\n",
        meta.title, started, meta.source_lang, meta.target_lang
    );
    for l in lines {
        let tr = l.translation.clone().unwrap_or_else(|| "_(sem tradução)_".into());
        out.push_str(&format!("**[{}] {}:** {}\n> {}\n\n", clock(l.t0), label_for(l, speakers), tr, l.text));
    }
    out
}

/// Encerra sessões sem `ended` (app fechou no meio). `ended` = início + último t1,
/// ou o próprio início se não houver falas.
pub fn recover_unfinished(root: &Path) -> Result<Vec<PathBuf>> {
    let mut done = vec![];
    let Ok(entries) = fs::read_dir(root) else { return Ok(done) };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(bytes) = fs::read(dir.join("meta.json")) else { continue };
        let Ok(mut meta) = serde_json::from_slice::<Meta>(&bytes) else { continue };
        if meta.ended.is_some() { continue; }
        // Uma sessão defeituosa não pode impedir a recuperação das demais.
        // Só o caminho é logado: o conteúdo das falas nunca vai para o log.
        let result = replay(&dir.join("lines.jsonl")).and_then(|lines| {
            meta.ended = Some(lines.iter().map(|l| l.t1).max().map_or(meta.started, |t| meta.started + t));
            close_session(&dir, &meta, &read_speakers(&dir))
        });
        match result {
            Ok(_) => done.push(dir),
            Err(e) => tracing::warn!(path = %dir.display(), error = %format!("{e:#}"), "sessão não recuperada"),
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn meta(id: &str) -> Meta {
        Meta {
            v: 1, id: id.into(), title: "Reunião fictícia".into(), started: 1_000,
            ended: None, source_lang: "en".into(), target_lang: "pt".into(),
            models: Default::default(), app_version: "0.1.0".into(),
        }
    }

    fn line(id: &str, t0: u64, text: &str, tr: Option<&str>) -> Line {
        Line { id: id.into(), t0, t1: t0 + 500, speaker: Speaker::system(),
               text: text.into(), translation: tr.map(Into::into), translation_error: false }
    }

    #[test]
    fn append_and_replay_keeps_order() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s1")).unwrap();
        w.append(&line("f1", 0, "Hello team.", None)).unwrap();
        w.append(&line("f2", 900, "Let's start.", None)).unwrap();
        let got = replay(&w.dir().join("lines.jsonl")).unwrap();
        assert_eq!(got.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["f1", "f2"]);
    }

    #[test]
    fn late_translation_updates_by_id_without_duplicate() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s2")).unwrap();
        w.append(&line("f1", 0, "Hello team.", None)).unwrap();
        w.append(&line("f2", 900, "Let's start.", None)).unwrap();
        w.append(&line("f1", 0, "Hello team.", Some("Olá, equipe."))).unwrap();
        let got = replay(&w.dir().join("lines.jsonl")).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, "f1");
        assert_eq!(got[0].translation.as_deref(), Some("Olá, equipe."));
    }

    #[test]
    fn truncated_last_record_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s3")).unwrap();
        w.append(&line("f1", 0, "Hello team.", Some("Olá, equipe."))).unwrap();
        let path = w.dir().join("lines.jsonl");
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"id":"f2","t0":9"#).unwrap();
        let got = replay(&path).unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn finish_writes_meta_and_markdown() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s4")).unwrap();
        w.append(&line("f1", 183_200, "Let's review the timeline.", Some("Vamos revisar o cronograma."))).unwrap();
        let mut me = line("f2", 190_000, "Sounds good.", None);
        me.speaker = Speaker::mic();
        me.translation_error = true;
        w.append(&me).unwrap();
        let dir = w.dir().to_path_buf();
        let md_path = w.finish(5_000).unwrap();
        let md = std::fs::read_to_string(md_path).unwrap();
        assert!(md.contains("**[00:03:03] Participante:** Vamos revisar o cronograma."));
        assert!(md.contains("> Let's review the timeline."));
        assert!(md.contains("**[00:03:10] Você:** _(sem tradução)_"));
        let m: Meta = serde_json::from_slice(&std::fs::read(dir.join("meta.json")).unwrap()).unwrap();
        assert_eq!(m.ended, Some(5_000));
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn recover_closes_unfinished_sessions() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s5")).unwrap();
        w.append(&line("f1", 2_000, "Hello team.", None)).unwrap();
        drop(w); // simula queda: sem finish
        let done = recover_unfinished(root.path()).unwrap();
        assert_eq!(done.len(), 1);
        let m: Meta = serde_json::from_slice(&std::fs::read(root.path().join("s5/meta.json")).unwrap()).unwrap();
        assert_eq!(m.ended, Some(1_000 + 2_500)); // started + último t1
        assert!(root.path().join("s5/transcript.md").exists());
        assert!(recover_unfinished(root.path()).unwrap().is_empty());
    }

    #[test]
    fn recover_session_without_lines_ends_at_start() {
        let root = tempfile::tempdir().unwrap();
        let w = SessionWriter::create(root.path(), meta("vazia")).unwrap();
        drop(w); // queda antes de qualquer fala
        let done = recover_unfinished(root.path()).unwrap();
        assert_eq!(done.len(), 1);
        let m: Meta = serde_json::from_slice(&std::fs::read(root.path().join("vazia/meta.json")).unwrap()).unwrap();
        assert_eq!(m.ended, Some(m.started));
    }

    #[test]
    fn truncated_multibyte_character_does_not_break_replay() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s7")).unwrap();
        w.append(&line("f1", 0, "Olá, equipe.", None)).unwrap();
        let full = serde_json::to_string(&line("f2", 900, "Olá", None)).unwrap();
        // Corta logo após o primeiro byte de "á" (2 bytes em UTF-8): queda no meio do caractere.
        let cut = full.find('á').unwrap() + 1;
        let path = w.dir().join("lines.jsonl");
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&full.as_bytes()[..cut]).unwrap();
        let got = replay(&path).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "f1");
        let done = recover_unfinished(root.path()).unwrap();
        assert_eq!(done.len(), 1);
    }

    #[test]
    fn bad_session_does_not_stop_recovery_of_others() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("good")).unwrap();
        w.append(&line("f1", 2_000, "Hello team.", None)).unwrap();
        drop(w);
        // Sessão ruim: meta válido e sem fim, mas lines.jsonl é um diretório (leitura falha).
        let bad = SessionWriter::create(root.path(), meta("bad")).unwrap();
        drop(bad);
        let bad_dir = root.path().join("bad");
        std::fs::remove_file(bad_dir.join("lines.jsonl")).unwrap();
        std::fs::create_dir(bad_dir.join("lines.jsonl")).unwrap();
        let done = recover_unfinished(root.path()).unwrap();
        assert_eq!(done, vec![root.path().join("good")]);
        assert!(root.path().join("good/transcript.md").exists());
    }

    #[test]
    fn session_id_format() {
        use chrono::TimeZone;
        let dt = chrono::Local.with_ymd_and_hms(2026, 10, 8, 14, 32, 10).unwrap();
        assert_eq!(session_id(dt), "2026-10-08_143210");
    }

    fn tracked(id: &str, t0: u64, voice: &str, label: &str, text: &str) -> Line {
        Line { speaker: Speaker::tracked(voice, label), ..line(id, t0, text, Some("ok")) }
    }

    fn speakers(pairs: &[(&str, &str, Option<&str>)]) -> Speakers {
        pairs.iter().map(|(v, l, id)| (v.to_string(), SpeakerEntry { label: l.to_string(), voice_id: id.map(Into::into) })).collect()
    }

    #[test]
    fn speakers_json_is_atomic_and_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp1")).unwrap();
        let s = speakers(&[("v1", "Ana", Some("abc-0")), ("v2", "Participante 2", None)]);
        w.set_speakers(s.clone()).unwrap();
        let dir = w.dir().to_path_buf();
        assert_eq!(read_speakers(&dir), s);
        let raw = std::fs::read_to_string(dir.join("speakers.json")).unwrap();
        assert!(raw.contains("\"voice_id\": \"abc-0\""));
        assert!(!raw.contains("\"voice_id\": null"));
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn markdown_uses_speakers_json_names_even_for_earlier_lines() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp2")).unwrap();
        w.append(&tracked("s0", 1_000, "v1", "Participante 1", "Hello team.")).unwrap();
        w.append(&tracked("s1", 2_000, "v2", "Participante 2", "Good morning.")).unwrap();
        w.append(&line("m0", 3_000, "Hi.", Some("Oi."))).unwrap();
        // Renomeado depois das falas: só o speakers.json muda.
        w.set_speakers(speakers(&[("v1", "Ana", Some("abc-0")), ("v2", "Participante 2", None)])).unwrap();
        let md = std::fs::read_to_string(w.finish(9_000).unwrap()).unwrap();
        assert!(md.contains("**[00:00:01] Ana:** ok"), "{md}");
        assert!(md.contains("**[00:00:02] Participante 2:** ok"), "{md}");
        assert!(md.contains("**[00:00:03] Participante:** Oi."), "{md}");
    }

    #[test]
    fn old_sessions_without_voice_or_speakers_json_still_work() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("antiga");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meta.json"), serde_json::to_vec(&meta("antiga")).unwrap()).unwrap();
        // Registro exatamente como a etapa 1 gravava: speaker sem `voice`.
        std::fs::write(dir.join("lines.jsonl"),
            "{\"id\":\"s0\",\"t0\":0,\"t1\":900,\"speaker\":{\"source\":\"system\",\"label\":\"Participante\"},\"text\":\"Hello team.\",\"translation\":\"Olá, equipe.\"}\n").unwrap();
        let lines = replay(&dir.join("lines.jsonl")).unwrap();
        assert_eq!(lines[0].speaker, Speaker::system());
        assert!(read_speakers(&dir).is_empty());
        recover_unfinished(root.path()).unwrap();
        let md = std::fs::read_to_string(dir.join("transcript.md")).unwrap();
        assert!(md.contains("**[00:00:00] Participante:** Olá, equipe."));
        // E a etapa 2 continua gravando sem `voice` para o microfone e o sistema genérico.
        assert!(!serde_json::to_string(&Speaker::mic()).unwrap().contains("voice"));
    }

    #[test]
    fn recovery_uses_names_from_speakers_json() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp3")).unwrap();
        w.append(&tracked("s0", 2_000, "v1", "Participante 1", "Hello team.")).unwrap();
        w.set_speakers(speakers(&[("v1", "Ana", Some("abc-0"))])).unwrap();
        drop(w); // queda: sem finish
        recover_unfinished(root.path()).unwrap();
        let md = std::fs::read_to_string(root.path().join("sp3/transcript.md")).unwrap();
        assert!(md.contains("**[00:00:02] Ana:** ok"), "{md}");
    }

    #[test]
    fn corrupt_speakers_json_falls_back_to_line_labels() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp4")).unwrap();
        w.append(&tracked("s0", 0, "v1", "Participante 1", "Hello team.")).unwrap();
        std::fs::write(w.dir().join("speakers.json"), "{ quebrado").unwrap();
        drop(w);
        recover_unfinished(root.path()).unwrap();
        let md = std::fs::read_to_string(root.path().join("sp4/transcript.md")).unwrap();
        assert!(md.contains("**[00:00:00] Participante 1:** ok"), "{md}");
    }

    #[test]
    fn missing_translation_is_omitted_from_json_and_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s6")).unwrap();
        w.append(&line("f1", 0, "Hello team.", None)).unwrap();
        let raw = std::fs::read_to_string(w.dir().join("lines.jsonl")).unwrap();
        assert!(!raw.contains("\"translation\""));
        let got = replay(&w.dir().join("lines.jsonl")).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].translation, None);
        assert_eq!(got[0].text, "Hello team.");
    }
}
