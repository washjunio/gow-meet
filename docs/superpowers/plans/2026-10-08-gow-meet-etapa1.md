# gow-meet Etapa 1 — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** App desktop (Mac primeiro, depois Windows) que captura o áudio da reunião e do microfone, transcreve em inglês, traduz para português numa janela flutuante e salva a transcrição localmente.

**Architecture:** Workspace Cargo com uma biblioteca `gow-meet-core` (Rust puro, testável sem interface) contendo sessão, segmentação, transcrição, tradução, pipeline e captura; um binário `gow-meet-cli` que roda o pipeline a partir de um arquivo WAV (usado no spike do Windows e para ver traduções no terminal cedo); e o app Tauri v2 (`src-tauri` + React/TS em `src`) que liga a captura real às janelas.

**Tech Stack:** Rust stable, Tauri v2, React 18 + Vite + TypeScript, `whisper-rs` 0.14 (whisper.cpp), `ct2rs` (CTranslate2) com Opus-MT, `cpal` (microfone e loopback WASAPI), `screencapturekit` (áudio do sistema no Mac), `hound` (WAV), `reqwest` + `sha2` (modelos), `serde`/`serde_json`, `crossbeam-channel`, `tracing`.

**Spec:** `docs/superpowers/specs/2026-10-08-gow-meet-etapa1-design.md`

## Global Constraints

- Plataforma: Mac primeiro (macOS 14+, desenvolvido em M2 8 GB), Windows em seguida (RTX 2050, 4 GB VRAM). Linux fora.
- Processamento 100% local. Nenhum serviço externo além do download de modelos do Hugging Face.
- O áudio nunca é gravado em disco. Só texto.
- Modelos com licença que permita uso em empresa. NLLB proibido.
- Direção de tradução é configuração (`source_lang`, `target_lang`), padrão `en` → `pt`.
- Sessões em `<data_dir>/gow-meet/sessions/<YYYY-MM-DD_HHMMSS>/` com `meta.json`, `lines.jsonl`, `transcript.md`.
- `meta.json` e `transcript.md` escritos de forma atômica (temporário + rename); `lines.jsonl` só acrescenta, com flush por registro.
- Logs nunca contêm texto de reunião.
- Testes e exemplos usam somente frases fictícias. Nada de nomes reais.
- Overlay: tradução em destaque, original menor embaixo, parcial em cinza.
- Speaker: microfone → `{"source":"mic","label":"Você"}`; sistema → `{"source":"system","label":"Participante"}`.

## Ajustes em relação à spec (para revisão)

1. **Detecção de voz:** a Task 3 começa com detector por energia (RMS) atrás de uma trait `VoiceDetector`. O Silero integrado ao whisper.cpp exige uma versão do `whisper-rs` cuja API ainda não está estável; ele entra como troca da implementação, sem mudar o segmentador, se a validação manual mostrar cortes ruins.
2. **Áudio do sistema no Mac:** ScreenCaptureKit (crate `screencapturekit`) em vez de Core Audio process taps. Há crate Rust madura; taps exigiriam bindings manuais. Pede a permissão "Gravação de Tela e Áudio do Sistema".
3. **NSPanel** (overlay sobre apps em tela cheia) fica para depois da primeira validação; a v1 usa janela `alwaysOnTop` + `visibleOnAllWorkspaces`.
4. **Modelo de tradução convertido:** o Opus-MT precisa ser convertido para CTranslate2 uma vez (Task 6) e publicado num repositório do Hugging Face da gow, de onde o app baixa.

## Review Focus

1. Queda do app no meio da escrita deixa a última linha do `lines.jsonl` truncada → a recuperação ignora só essa linha e preserva as demais (teste na Task 2).
2. Tradução chega depois da fala fechada, ou de falas fora de ordem → a fala é atualizada pelo `id`, sem duplicar, no arquivo e no Markdown (teste na Task 2 e Task 7).
3. Monólogo longo sem pausa (> 25 s) → o segmentador força o fechamento antes do limite de 30 s do Whisper (teste na Task 3).
4. Whisper "alucina" em silêncio ou ruído (`[BLANK_AUDIO]`, `(music)`, "Thank you.") → fala vazia ou só marcador é descartada, não vira linha (teste na Task 5).
5. Você e o participante falando ao mesmo tempo → fluxos independentes, `id`s distintos, sem misturar textos (teste na Task 7).

## Estrutura de arquivos

```text
gow-meet/
  Cargo.toml                      workspace
  package.json, vite.config.ts, tsconfig.json, index.html
  .github/workflows/ci.yml
  crates/core/
    Cargo.toml
    src/lib.rs                    reexporta módulos
    src/session.rs                tipos Line/Speaker/Meta, SessionWriter, replay, markdown, recuperação
    src/segmenter.rs              VoiceDetector, RmsDetector, Segmenter (parciais e fechamento)
    src/resample.rs               conversão para 16 kHz mono
    src/wer.rs                    taxa de erro de palavras (testes de regressão)
    src/models.rs                 manifesto, download e verificação de hash
    src/asr.rs                    trait Transcriber + WhisperTranscriber + filtro de alucinações
    src/translate.rs              trait Translator + Ct2Translator
    src/pipeline.rs               liga tudo, limita parciais, descarta sob atraso, emite eventos
    src/capture/mod.rs            trait AudioSource, AudioChunk
    src/capture/mic.rs            microfone via cpal (todas as plataformas)
    src/capture/mac.rs            áudio do sistema via ScreenCaptureKit
    src/capture/windows.rs        áudio do sistema via loopback WASAPI (cpal)
    tests/fixtures/*.wav          frases fictícias geradas com `say` (commitadas)
    tests/fixtures/*.txt          texto esperado
  crates/cli/src/main.rs          gow-meet-cli: WAV → pipeline → terminal + métricas
  scripts/gen-fixtures.sh         gera os WAVs com `say`
  scripts/convert-opus-mt.sh      converte Opus-MT para CTranslate2
  src-tauri/                      app Tauri (comandos, janelas, estado)
  src/                            React: main window e overlay
  docs/spike-windows.md           resultado do spike
  docs/validacao-mac.md           resultado da validação manual
```

---
### Task 1: Toolchain, workspace, esqueleto Tauri e CI

**Files:**
- Create: `Cargo.toml`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `.gitignore`, `.github/workflows/ci.yml`
- Create (gerados): `package.json`, `vite.config.ts`, `tsconfig*.json`, `index.html`, `src/*`, `src-tauri/*`

**Interfaces:**
- Produces: crate `gow_meet_core` (lib) e binário `gow-meet-cli`; app Tauri em `src-tauri` com nome de crate `gow-meet-app`.

- [ ] **Step 1: Instalar Rust e CMake (Mac)**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
brew install cmake
rustc --version && cmake --version
```
Expected: versões impressas. CMake é exigido por `whisper-rs` e `ct2rs`.

- [ ] **Step 2: Criar o workspace e a crate core com um teste de fumaça**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/core", "crates/cli", "src-tauri"]

[workspace.package]
edition = "2021"
version = "0.1.0"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "1"
tracing = "0.1"
crossbeam-channel = "0.5"
```

`crates/core/Cargo.toml`:
```toml
[package]
name = "gow-meet-core"
edition.workspace = true
version.workspace = true

[lib]
name = "gow_meet_core"

[dependencies]
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
thiserror.workspace = true
tracing.workspace = true
crossbeam-channel.workspace = true

[dev-dependencies]
tempfile = "3"
```

`crates/core/src/lib.rs`:
```rust
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert_eq!(super::version(), "0.1.0");
    }
}
```

`crates/cli/Cargo.toml`:
```toml
[package]
name = "gow-meet-cli"
edition.workspace = true
version.workspace = true

[dependencies]
gow-meet-core = { path = "../core" }
anyhow.workspace = true
```

`crates/cli/src/main.rs`:
```rust
fn main() {
    println!("gow-meet-cli {}", gow_meet_core::version());
}
```

- [ ] **Step 3: Gerar o front-end React + Vite na raiz**

```bash
cd ~/Projects/gow-meet
npm create vite@5 web-tmp -- --template react-ts
cp -R web-tmp/. . && rm -rf web-tmp
npm install
```
Expected: `package.json`, `index.html`, `src/` na raiz; `npm run build` gera `dist/`.

- [ ] **Step 4: Inicializar o Tauri v2**

```bash
npm install -D @tauri-apps/cli@^2
npm install @tauri-apps/api@^2
npx tauri init --ci -A gow-meet -W gow-meet -D ../dist -P http://localhost:5173 \
  --before-dev-command "npm run dev" --before-build-command "npm run build"
```
Em `src-tauri/Cargo.toml`, troque `name` para `gow-meet-app` e acrescente `gow-meet-core = { path = "../crates/core" }` em `[dependencies]`.

- [ ] **Step 5: `.gitignore`**

```gitignore
/target
/dist
/node_modules
src-tauri/gen/schemas
*.log
/models
.DS_Store
```

- [ ] **Step 6: Verificar build e testes**

```bash
cargo test -p gow-meet-core
cargo run -p gow-meet-cli
npm run build
npx tauri dev   # abre uma janela vazia; feche-a
```
Expected: teste `version_is_set` PASS; CLI imprime `gow-meet-cli 0.1.0`; janela abre.

- [ ] **Step 7: CI em macOS e Windows**

`.github/workflows/ci.yml`:
```yaml
name: ci
on: [push, pull_request]
jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-14, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - run: npm ci
      - run: npm run build
      - run: cargo test --workspace
```
Os runners não têm GPU: o CI cobre lógica e compilação; desempenho fica no spike e na validação manual.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "Cria workspace, esqueleto Tauri e CI"
```

---
### Task 2: Sessão — formato, gravação, Markdown e recuperação

**Files:**
- Create: `crates/core/src/session.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml`
- Test: testes unitários no próprio `session.rs`

**Interfaces:**
- Produces:
  - `SpeakerSource { Mic, System }` (serde `"mic"`/`"system"`), `Speaker { source, label }`, `Speaker::mic()`, `Speaker::system()`
  - `Line { id: String, t0: u64, t1: u64, speaker: Speaker, text: String, translation: Option<String>, translation_error: bool }`
  - `Meta { v, id, title, started: u64, ended: Option<u64>, source_lang, target_lang, models: BTreeMap<String,String>, app_version }` (tempos em ms Unix)
  - `session_id(DateTime<Local>) -> String`
  - `SessionWriter::create(root: &Path, meta: Meta) -> Result<SessionWriter>`, `.append(&mut self, &Line) -> Result<()>`, `.dir() -> &Path`, `.finish(self, ended_ms: u64) -> Result<PathBuf>` (retorna o caminho do `transcript.md`)
  - `replay(lines_path: &Path) -> Result<Vec<Line>>`, `render_markdown(&Meta, &[Line]) -> String`, `recover_unfinished(root: &Path, now_ms: u64) -> Result<Vec<PathBuf>>`, `write_atomic(path: &Path, bytes: &[u8]) -> Result<()>`

- [ ] **Step 1: Dependência de data/hora**

Em `crates/core/Cargo.toml`, `[dependencies]`: `chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }`.
Em `lib.rs`, acrescente `pub mod session;`.

- [ ] **Step 2: Escrever os testes (falham por falta de implementação)**

No fim de `crates/core/src/session.rs`:
```rust
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
        let done = recover_unfinished(root.path(), 99_000).unwrap();
        assert_eq!(done.len(), 1);
        let m: Meta = serde_json::from_slice(&std::fs::read(root.path().join("s5/meta.json")).unwrap()).unwrap();
        assert_eq!(m.ended, Some(1_000 + 2_500)); // started + último t1
        assert!(root.path().join("s5/transcript.md").exists());
        assert!(recover_unfinished(root.path(), 99_000).unwrap().is_empty());
    }

    #[test]
    fn session_id_format() {
        use chrono::TimeZone;
        let dt = chrono::Local.with_ymd_and_hms(2026, 10, 8, 14, 32, 10).unwrap();
        assert_eq!(session_id(dt), "2026-10-08_143210");
    }
}
```

- [ ] **Step 3: Rodar e confirmar a falha**

Run: `cargo test -p gow-meet-core session`
Expected: erro de compilação (tipos não definidos).

- [ ] **Step 4: Implementação**

No início de `crates/core/src/session.rs`:
```rust
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
pub struct Speaker { pub source: SpeakerSource, pub label: String }

impl Speaker {
    pub fn mic() -> Self { Self { source: SpeakerSource::Mic, label: "Você".into() } }
    pub fn system() -> Self { Self { source: SpeakerSource::System, label: "Participante".into() } }
}

fn is_false(b: &bool) -> bool { !*b }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub id: String,
    pub t0: u64,
    pub t1: u64,
    pub speaker: Speaker,
    pub text: String,
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

pub struct SessionWriter { dir: PathBuf, meta: Meta, lines: File }

impl SessionWriter {
    pub fn create(root: &Path, meta: Meta) -> Result<Self> {
        let dir = root.join(&meta.id);
        fs::create_dir_all(&dir).with_context(|| format!("criar {}", dir.display()))?;
        write_meta(&dir, &meta)?;
        let lines = OpenOptions::new().create(true).append(true).open(dir.join("lines.jsonl"))?;
        Ok(Self { dir, meta, lines })
    }

    pub fn dir(&self) -> &Path { &self.dir }

    pub fn append(&mut self, line: &Line) -> Result<()> {
        let mut buf = serde_json::to_vec(line)?;
        buf.push(b'\n');
        self.lines.write_all(&buf)?;
        self.lines.flush()?;
        Ok(())
    }

    pub fn finish(mut self, ended_ms: u64) -> Result<PathBuf> {
        self.lines.sync_all()?;
        self.meta.ended = Some(ended_ms);
        close_session(&self.dir, &self.meta)
    }
}

fn close_session(dir: &Path, meta: &Meta) -> Result<PathBuf> {
    write_meta(dir, meta)?;
    let lines = replay(&dir.join("lines.jsonl"))?;
    let md = dir.join("transcript.md");
    write_atomic(&md, render_markdown(meta, &lines).as_bytes())?;
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
    for raw in BufReader::new(file).lines() {
        let raw = raw?;
        let Ok(line) = serde_json::from_str::<Line>(&raw) else {
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

pub fn render_markdown(meta: &Meta, lines: &[Line]) -> String {
    use chrono::TimeZone;
    let started = chrono::Local.timestamp_millis_opt(meta.started as i64).single()
        .map(|d| d.format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default();
    let mut out = format!(
        "# {}\n\n- Início: {}\n- Idiomas: {} → {}\n\n",
        meta.title, started, meta.source_lang, meta.target_lang
    );
    for l in lines {
        let tr = l.translation.clone().unwrap_or_else(|| "_(sem tradução)_".into());
        out.push_str(&format!("**[{}] {}:** {}\n> {}\n\n", clock(l.t0), l.speaker.label, tr, l.text));
    }
    out
}

/// Encerra sessões sem `ended` (app fechou no meio). `ended` = início + último t1,
/// ou `now_ms` se não houver falas.
pub fn recover_unfinished(root: &Path, now_ms: u64) -> Result<Vec<PathBuf>> {
    let mut done = vec![];
    let Ok(entries) = fs::read_dir(root) else { return Ok(done) };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(bytes) = fs::read(dir.join("meta.json")) else { continue };
        let Ok(mut meta) = serde_json::from_slice::<Meta>(&bytes) else { continue };
        if meta.ended.is_some() { continue; }
        let lines = replay(&dir.join("lines.jsonl"))?;
        meta.ended = Some(lines.iter().map(|l| l.t1).max().map_or(now_ms, |t| meta.started + t));
        close_session(&dir, &meta)?;
        done.push(dir);
    }
    Ok(done)
}
```

- [ ] **Step 5: Rodar os testes**

Run: `cargo test -p gow-meet-core session`
Expected: 6 testes PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "Adiciona gravação de sessões, Markdown e recuperação"
```

---
### Task 3: Reamostragem e segmentação de falas

**Files:**
- Create: `crates/core/src/resample.rs`, `crates/core/src/segmenter.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod resample; pub mod segmenter;`)

**Interfaces:**
- Produces:
  - `pub const SAMPLE_RATE: u32 = 16_000;` (em `resample.rs`)
  - `to_mono(interleaved: &[f32], channels: usize) -> Vec<f32>`
  - `Resampler::new(in_rate: u32, out_rate: u32)`, `.process(&mut self, input: &[f32]) -> Vec<f32>`
  - `trait VoiceDetector: Send { fn is_speech(&mut self, frame: &[f32]) -> bool; }`, `RmsDetector { threshold: f32 }`
  - `SegmenterConfig` (com `Default`), `Segmenter::new(cfg, Box<dyn VoiceDetector>)`, `.push(&mut self, samples: &[f32]) -> Vec<SegmentEvent>`, `.flush(&mut self) -> Vec<SegmentEvent>`
  - `enum SegmentEvent { Partial { seq: u64, start_ms: u64, audio: Vec<f32> }, Final { seq: u64, start_ms: u64, end_ms: u64, audio: Vec<f32> } }`

- [ ] **Step 1: Testes da reamostragem**

Em `crates/core/src/resample.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin())
            .collect()
    }

    #[test]
    fn downmix_stereo_averages_channels() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resample_48k_to_16k_in_chunks_keeps_duration() {
        let input = sine(48_000, 1.0);
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = vec![];
        for chunk in input.chunks(1_000) { out.extend(r.process(chunk)); }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len = {}", out.len());
        assert!(out.iter().all(|s| s.abs() <= 1.0001));
    }

    #[test]
    fn resample_44100_to_16k_keeps_duration() {
        let mut r = Resampler::new(44_100, 16_000);
        let out = r.process(&sine(44_100, 2.0));
        assert!((out.len() as i64 - 32_000).abs() <= 2, "len = {}", out.len());
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p gow-meet-core resample`
Expected: erro de compilação.

- [ ] **Step 3: Implementar a reamostragem**

No topo de `resample.rs`:
```rust
pub const SAMPLE_RATE: u32 = 16_000;

pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 { return interleaved.to_vec(); }
    interleaved.chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Interpolação linear em fluxo contínuo. Suficiente para voz; não aplica
/// filtro passa-baixa (a fala útil fica abaixo de 8 kHz).
pub struct Resampler { step: f64, pos: f64, last: Option<f32> }

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self { step: in_rate as f64 / out_rate as f64, pos: 0.0, last: None }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() { return vec![]; }
        let mut buf = Vec::with_capacity(input.len() + 1);
        if let Some(l) = self.last { buf.push(l); }
        buf.extend_from_slice(input);
        let end = (buf.len() - 1) as f64;
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        let mut t = self.pos;
        while t <= end {
            let i = t.floor() as usize;
            let f = (t - i as f64) as f32;
            let b = if i + 1 < buf.len() { buf[i + 1] } else { buf[i] };
            out.push(buf[i] * (1.0 - f) + b * f);
            t += self.step;
        }
        self.pos = t - end;
        self.last = buf.last().copied();
        out
    }
}
```

- [ ] **Step 4: Rodar os testes da reamostragem**

Run: `cargo test -p gow-meet-core resample`
Expected: 3 PASS.

- [ ] **Step 5: Testes do segmentador**

Em `crates/core/src/segmenter.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MS: usize = 16; // amostras por ms a 16 kHz

    fn tone(ms: usize) -> Vec<f32> {
        (0..ms * MS).map(|i| 0.3 * (i as f32 * 0.17).sin()).collect()
    }
    fn silence(ms: usize) -> Vec<f32> { vec![0.0; ms * MS] }

    fn seg() -> Segmenter {
        Segmenter::new(SegmenterConfig::default(), Box::new(RmsDetector { threshold: 0.01 }))
    }

    fn finals(ev: &[SegmentEvent]) -> Vec<(u64, u64, u64)> {
        ev.iter().filter_map(|e| match e {
            SegmentEvent::Final { seq, start_ms, end_ms, .. } => Some((*seq, *start_ms, *end_ms)),
            _ => None,
        }).collect()
    }

    #[test]
    fn silence_produces_nothing() {
        let mut s = seg();
        assert!(s.push(&silence(5_000)).is_empty());
        assert!(s.flush().is_empty());
    }

    #[test]
    fn speech_then_pause_closes_one_utterance() {
        let mut s = seg();
        let mut ev = s.push(&silence(500));
        ev.extend(s.push(&tone(1_500)));
        ev.extend(s.push(&silence(1_000)));
        let f = finals(&ev);
        assert_eq!(f.len(), 1);
        let (seq, start, end) = f[0];
        assert_eq!(seq, 0);
        assert!((280..=500).contains(&start), "start = {start}"); // 200 ms de pré-roll
        assert!((2_000..=2_700).contains(&end), "end = {end}");
    }

    #[test]
    fn long_speech_emits_partials_with_same_seq() {
        let mut s = seg();
        let ev = s.push(&tone(3_500));
        let partials: Vec<u64> = ev.iter().filter_map(|e| match e {
            SegmentEvent::Partial { seq, .. } => Some(*seq), _ => None }).collect();
        assert!(partials.len() >= 3, "parciais = {}", partials.len());
        assert!(partials.iter().all(|&q| q == 0));
    }

    #[test]
    fn monologue_is_force_closed_before_whisper_limit() {
        let mut s = seg();
        let ev = s.push(&tone(40_000));
        let f = finals(&ev);
        assert_eq!(f.len(), 1);
        assert!(f[0].2 - f[0].1 <= 25_000);
        let rest = finals(&s.flush());
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].0, 1);
    }

    #[test]
    fn short_click_is_discarded() {
        let mut s = seg();
        let mut ev = s.push(&tone(60));
        ev.extend(s.push(&silence(1_000)));
        assert!(finals(&ev).is_empty());
    }

    #[test]
    fn odd_chunk_sizes_do_not_lose_samples() {
        let mut s = seg();
        let audio: Vec<f32> = [tone(1_000), silence(1_000)].concat();
        let mut ev = vec![];
        for c in audio.chunks(333) { ev.extend(s.push(c)); }
        assert_eq!(finals(&ev).len(), 1);
    }
}
```

- [ ] **Step 6: Rodar e ver falhar**

Run: `cargo test -p gow-meet-core segmenter`
Expected: erro de compilação.

- [ ] **Step 7: Implementar o segmentador**

No topo de `segmenter.rs`:
```rust
use std::collections::VecDeque;

pub trait VoiceDetector: Send {
    fn is_speech(&mut self, frame: &[f32]) -> bool;
}

/// Detector por energia. Substituível pelo Silero sem mudar o `Segmenter`.
pub struct RmsDetector { pub threshold: f32 }

impl VoiceDetector for RmsDetector {
    fn is_speech(&mut self, frame: &[f32]) -> bool {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt();
        rms > self.threshold
    }
}

#[derive(Debug, Clone)]
pub struct SegmenterConfig {
    pub frame_ms: u64,
    pub preroll_ms: u64,
    pub partial_every_ms: u64,
    pub silence_close_ms: u64,
    pub min_speech_ms: u64,
    pub max_utterance_ms: u64,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self { frame_ms: 30, preroll_ms: 200, partial_every_ms: 1_000,
               silence_close_ms: 600, min_speech_ms: 300, max_utterance_ms: 25_000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SegmentEvent {
    Partial { seq: u64, start_ms: u64, audio: Vec<f32> },
    Final { seq: u64, start_ms: u64, end_ms: u64, audio: Vec<f32> },
}

pub struct Segmenter {
    cfg: SegmenterConfig,
    vad: Box<dyn VoiceDetector>,
    frame_len: usize,
    pending: Vec<f32>,
    preroll: VecDeque<Vec<f32>>,
    clock_ms: u64,
    active: bool,
    buf: Vec<f32>,
    start_ms: u64,
    speech_ms: u64,
    silence_ms: u64,
    since_partial_ms: u64,
    seq: u64,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig, vad: Box<dyn VoiceDetector>) -> Self {
        let frame_len = (cfg.frame_ms * 16) as usize;
        Self { cfg, vad, frame_len, pending: vec![], preroll: VecDeque::new(), clock_ms: 0,
               active: false, buf: vec![], start_ms: 0, speech_ms: 0, silence_ms: 0,
               since_partial_ms: 0, seq: 0 }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<SegmentEvent> {
        self.pending.extend_from_slice(samples);
        let mut out = vec![];
        while self.pending.len() >= self.frame_len {
            let frame: Vec<f32> = self.pending.drain(..self.frame_len).collect();
            self.step(frame, &mut out);
        }
        out
    }

    pub fn flush(&mut self) -> Vec<SegmentEvent> {
        let mut out = vec![];
        if self.active { self.close(&mut out); }
        out
    }

    fn step(&mut self, frame: Vec<f32>, out: &mut Vec<SegmentEvent>) {
        let fm = self.cfg.frame_ms;
        let speech = self.vad.is_speech(&frame);
        self.clock_ms += fm;
        if !self.active {
            if speech {
                self.active = true;
                let pre: Vec<f32> = self.preroll.drain(..).flatten().collect();
                self.start_ms = self.clock_ms - fm - (pre.len() as u64 / 16);
                self.buf = pre;
                self.buf.extend_from_slice(&frame);
                self.speech_ms = fm;
                self.silence_ms = 0;
                self.since_partial_ms = 0;
            } else {
                self.preroll.push_back(frame);
                while self.preroll.len() as u64 * fm > self.cfg.preroll_ms { self.preroll.pop_front(); }
            }
            return;
        }
        self.buf.extend_from_slice(&frame);
        self.since_partial_ms += fm;
        if speech { self.speech_ms += fm; self.silence_ms = 0; } else { self.silence_ms += fm; }
        let len_ms = self.buf.len() as u64 / 16;
        if self.silence_ms >= self.cfg.silence_close_ms || len_ms >= self.cfg.max_utterance_ms {
            self.close(out);
        } else if self.since_partial_ms >= self.cfg.partial_every_ms {
            self.since_partial_ms = 0;
            out.push(SegmentEvent::Partial { seq: self.seq, start_ms: self.start_ms, audio: self.buf.clone() });
        }
    }

    fn close(&mut self, out: &mut Vec<SegmentEvent>) {
        self.active = false;
        let audio = std::mem::take(&mut self.buf);
        if self.speech_ms >= self.cfg.min_speech_ms {
            let end_ms = self.start_ms + audio.len() as u64 / 16;
            out.push(SegmentEvent::Final { seq: self.seq, start_ms: self.start_ms, end_ms, audio });
            self.seq += 1;
        }
    }
}
```

- [ ] **Step 8: Rodar os testes**

Run: `cargo test -p gow-meet-core segmenter resample`
Expected: todos PASS. Se `speech_then_pause_closes_one_utterance` falhar só pelos limites de `start`/`end`, imprima os valores e confira o cálculo do pré-roll antes de alargar a faixa.

- [ ] **Step 9: Commit**

```bash
git add crates/core
git commit -m "Adiciona reamostragem e segmentação de falas"
```

---
### Task 4: Modelos — manifesto, download e verificação

**Files:**
- Create: `crates/core/src/models.rs`, `crates/core/models.json`, `scripts/convert-opus-mt.sh`
- Modify: `crates/core/src/lib.rs` (`pub mod models;`), `crates/core/Cargo.toml`

**Interfaces:**
- Produces:
  - `ModelFile { name: String, path: String, sha256: String }`, `ModelSpec { files: Vec<ModelFile> }`
  - `manifest() -> BTreeMap<String, ModelSpec>` (lê `models.json` embutido)
  - `trait Fetch { fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)>; }`, `HttpFetch`
  - `ensure(spec: &ModelSpec, base_url: &str, dir: &Path, fetch: &dyn Fetch, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf>` — devolve `dir` com todos os arquivos verificados
  - `DEFAULT_BASE_URL: &str = "https://huggingface.co"`
  - IDs do manifesto: `"whisper-small-en"`, `"whisper-turbo"`, `"opus-mt-en-pt"`

- [ ] **Step 1: Dependências**

`crates/core/Cargo.toml`, `[dependencies]`:
```toml
sha2 = "0.10"
hex = "0.4"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }
```

- [ ] **Step 2: Converter o Opus-MT e publicar no Hugging Face da gow (uma vez, manual)**

`scripts/convert-opus-mt.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
OUT="${1:-models/opus-mt-en-pt-ct2}"
python3 -m venv .venv-convert
. .venv-convert/bin/activate
pip install "ctranslate2>=4.4" "transformers[sentencepiece]>=4.44" torch
ct2-transformers-converter --model Helsinki-NLP/opus-mt-tc-big-en-pt \
  --output_dir "$OUT" --quantization int8 --copy_files source.spm target.spm
ls -la "$OUT"
```
Rode `bash scripts/convert-opus-mt.sh`. Confira no model card de `Helsinki-NLP/opus-mt-tc-big-en-pt` se o modelo exige o prefixo de idioma de destino (ex.: `>>por<<`); anote o valor para a Task 6.
Crie no Hugging Face um repositório de modelo da gow (nome proposto: `gow-solucoes/opus-mt-en-pt-ct2`; ajuste ao nome real da organização) e envie os arquivos de `models/opus-mt-en-pt-ct2/`. Mantenha a atribuição CC-BY 4.0 no README do repositório.

- [ ] **Step 3: Calcular os hashes e escrever o manifesto**

```bash
mkdir -p models/tmp && cd models/tmp
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin
shasum -a 256 *.bin ../opus-mt-en-pt-ct2/*
```
`crates/core/models.json` (substitua cada `sha256` pelo valor impresso acima — a etapa só termina com os hashes reais):
```json
{
  "whisper-small-en": { "files": [
    { "name": "ggml-small.en-q5_1.bin", "path": "ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin", "sha256": "<valor do shasum>" }
  ]},
  "whisper-turbo": { "files": [
    { "name": "ggml-large-v3-turbo-q5_0.bin", "path": "ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin", "sha256": "<valor do shasum>" }
  ]},
  "opus-mt-en-pt": { "files": [
    { "name": "model.bin", "path": "gow-solucoes/opus-mt-en-pt-ct2/resolve/main/model.bin", "sha256": "<valor do shasum>" },
    { "name": "config.json", "path": "gow-solucoes/opus-mt-en-pt-ct2/resolve/main/config.json", "sha256": "<valor do shasum>" },
    { "name": "shared_vocabulary.json", "path": "gow-solucoes/opus-mt-en-pt-ct2/resolve/main/shared_vocabulary.json", "sha256": "<valor do shasum>" },
    { "name": "source.spm", "path": "gow-solucoes/opus-mt-en-pt-ct2/resolve/main/source.spm", "sha256": "<valor do shasum>" },
    { "name": "target.spm", "path": "gow-solucoes/opus-mt-en-pt-ct2/resolve/main/target.spm", "sha256": "<valor do shasum>" }
  ]}
}
```
Se a conversão gerar arquivos com nomes diferentes (ex.: `source_vocabulary.json` e `target_vocabulary.json` em vez de `shared_vocabulary.json`), liste exatamente os arquivos de `ls` do Step 2.

- [ ] **Step 4: Testes com um `Fetch` falso**

Em `crates/core/src/models.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    struct FakeFetch { files: HashMap<String, Vec<u8>>, calls: Cell<usize> }
    impl Fetch for FakeFetch {
        fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)> {
            self.calls.set(self.calls.get() + 1);
            let data = self.files.get(url).cloned().ok_or_else(|| anyhow::anyhow!("404 {url}"))?;
            let len = data.len() as u64;
            Ok((Box::new(std::io::Cursor::new(data)), Some(len)))
        }
    }

    fn sha(b: &[u8]) -> String { hex::encode(Sha256::digest(b)) }

    fn spec(sha256: String) -> ModelSpec {
        ModelSpec { files: vec![ModelFile { name: "m.bin".into(), path: "org/m.bin".into(), sha256 }] }
    }

    fn fetch() -> FakeFetch {
        FakeFetch { files: HashMap::from([("https://hf.test/org/m.bin".to_string(), b"modelo".to_vec())]),
                    calls: Cell::new(0) }
    }

    #[test]
    fn downloads_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let f = fetch();
        let mut seen = 0;
        ensure(&spec(sha(b"modelo")), "https://hf.test", dir.path(), &f, &mut |n, _| seen = n).unwrap();
        assert_eq!(std::fs::read(dir.path().join("m.bin")).unwrap(), b"modelo");
        assert_eq!(seen, 6);
    }

    #[test]
    fn skips_valid_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.bin"), b"modelo").unwrap();
        let f = fetch();
        ensure(&spec(sha(b"modelo")), "https://hf.test", dir.path(), &f, &mut |_, _| {}).unwrap();
        assert_eq!(f.calls.get(), 0);
    }

    #[test]
    fn wrong_hash_fails_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let err = ensure(&spec(sha(b"outro")), "https://hf.test", dir.path(), &fetch(), &mut |_, _| {});
        assert!(err.is_err());
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn corrupt_existing_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.bin"), b"corrompido").unwrap();
        ensure(&spec(sha(b"modelo")), "https://hf.test", dir.path(), &fetch(), &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.path().join("m.bin")).unwrap(), b"modelo");
    }

    #[test]
    fn manifest_has_expected_ids_and_real_hashes() {
        let m = manifest();
        for id in ["whisper-small-en", "whisper-turbo", "opus-mt-en-pt"] {
            let spec = m.get(id).unwrap_or_else(|| panic!("falta {id}"));
            assert!(spec.files.iter().all(|f| f.sha256.len() == 64), "hash ausente em {id}");
        }
    }
}
```

- [ ] **Step 5: Rodar e ver falhar**

Run: `cargo test -p gow-meet-core models`
Expected: erro de compilação.

- [ ] **Step 6: Implementar**

No topo de `models.rs`:
```rust
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_BASE_URL: &str = "https://huggingface.co";

#[derive(Debug, Clone, Deserialize)]
pub struct ModelFile { pub name: String, pub path: String, pub sha256: String }

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSpec { pub files: Vec<ModelFile> }

pub fn manifest() -> BTreeMap<String, ModelSpec> {
    serde_json::from_str(include_str!("../models.json")).expect("models.json válido")
}

pub trait Fetch {
    fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)>;
}

pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)> {
        let resp = reqwest::blocking::Client::builder()
            .timeout(None)
            .build()?
            .get(url)
            .send()?
            .error_for_status()
            .with_context(|| format!("baixar {url}"))?;
        let len = resp.content_length();
        Ok((Box::new(resp), len))
    }
}

fn file_sha(path: &Path) -> Result<String> {
    let mut h = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

pub fn ensure(
    spec: &ModelSpec, base_url: &str, dir: &Path, fetch: &dyn Fetch,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    for f in &spec.files {
        let dest = dir.join(&f.name);
        if dest.exists() && file_sha(&dest)? == f.sha256 { continue; }
        let part = dir.join(format!("{}.part", f.name));
        let url = format!("{}/{}", base_url.trim_end_matches('/'), f.path);
        let (mut body, total) = fetch.open(&url)?;
        let mut out = File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut done = 0u64;
        loop {
            let n = body.read(&mut buf)?;
            if n == 0 { break; }
            out.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            done += n as u64;
            progress(done, total);
        }
        out.sync_all()?;
        drop(out);
        let got = hex::encode(hasher.finalize());
        if got != f.sha256 {
            let _ = fs::remove_file(&part);
            bail!("hash inválido para {}: esperado {}, obtido {}", f.name, f.sha256, got);
        }
        fs::rename(&part, &dest)?;
    }
    Ok(dir.to_path_buf())
}
```

- [ ] **Step 7: Rodar os testes**

Run: `cargo test -p gow-meet-core models`
Expected: 5 PASS (o último só passa com os hashes reais do Step 3).

- [ ] **Step 8: Commit**

```bash
git add crates/core scripts/convert-opus-mt.sh
git commit -m "Adiciona manifesto e download verificado de modelos"
```

---
### Task 5: Transcrição com whisper.cpp, WER e fixtures sintéticas

**Files:**
- Create: `crates/core/src/asr.rs`, `crates/core/src/wer.rs`, `crates/core/src/wav.rs`, `scripts/gen-fixtures.sh`, `crates/core/tests/fixtures/*.wav|*.txt`, `crates/core/tests/asr_whisper.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod asr; pub mod wer; pub mod wav;`), `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `resample::{to_mono, Resampler, SAMPLE_RATE}` (Task 3)
- Produces:
  - `trait Transcriber: Send { fn transcribe(&mut self, audio: &[f32]) -> anyhow::Result<String>; }` — áudio 16 kHz mono; devolve texto bruto
  - `clean_transcript(raw: &str) -> Option<String>` — remove marcadores e alucinações; `None` se nada sobrar
  - `WhisperTranscriber::new(model_path: &Path, lang: &str, use_gpu: bool) -> Result<Self>`
  - `wer(reference: &str, hypothesis: &str) -> f32`
  - `read_wav_16k_mono(path: &Path) -> Result<Vec<f32>>`

- [ ] **Step 1: Dependências e features de GPU**

`crates/core/Cargo.toml`:
```toml
[dependencies]
hound = "3.5"

[target.'cfg(target_os = "macos")'.dependencies]
whisper-rs = { version = "0.14", features = ["metal"] }

[target.'cfg(not(target_os = "macos"))'.dependencies]
whisper-rs = { version = "0.14" }

[features]
vulkan = ["whisper-rs/vulkan"]
cuda = ["whisper-rs/cuda"]
```
Se `cargo build` reclamar de nome de feature, confira as features da versão resolvida em `docs.rs/crate/whisper-rs/<versão>/features` e ajuste só os nomes.

- [ ] **Step 2: Gerar fixtures sintéticas**

`scripts/gen-fixtures.sh`:
```bash
#!/usr/bin/env bash
# Gera áudio fictício em inglês com a voz do macOS. Só frases inventadas.
set -euo pipefail
OUT=crates/core/tests/fixtures
mkdir -p "$OUT"
say_wav() { say -v Samantha --data-format=LEI16@16000 -o "$OUT/$1.wav" "$2"; printf '%s\n' "$3" > "$OUT/$1.txt"; }
say_wav s1 "Let's review the project timeline for next week." "Let's review the project timeline for next week."
say_wav s2 "The new dashboard should be ready by Friday." "The new dashboard should be ready by Friday."
say_wav s3 "Can you share the budget estimate with the team?" "Can you share the budget estimate with the team?"
say_wav s4 "We need to decide which features go into the first release." "We need to decide which features go into the first release."
say_wav s5 "I will send a summary after this call." "I will send a summary after this call."
say -v Samantha --data-format=LEI16@16000 -o "$OUT/meeting.wav" \
  "Good morning everyone. [[slnc 1500]] Let's review the project timeline for next week. [[slnc 1500]] \
   The new dashboard should be ready by Friday. [[slnc 1500]] Can you share the budget estimate with the team? \
   [[slnc 1500]] I will send a summary after this call."
ls -la "$OUT"
```
Run: `bash scripts/gen-fixtures.sh`. Expected: `s1..s5.wav/.txt` e `meeting.wav` (~25 s).

- [ ] **Step 3: Testes de WER, limpeza e leitura de WAV**

`crates/core/src/wer.rs`, no fim:
```rust
#[cfg(test)]
mod tests {
    use super::wer;
    #[test]
    fn identical_ignoring_case_and_punctuation() {
        assert_eq!(wer("Let's review the timeline.", "let's review the timeline"), 0.0);
    }
    #[test]
    fn one_substitution_in_four_words() {
        assert!((wer("ready by next friday", "ready by last friday") - 0.25).abs() < 1e-6);
    }
    #[test]
    fn empty_hypothesis_is_full_error() {
        assert_eq!(wer("share the budget", ""), 1.0);
    }
}
```

`crates/core/src/asr.rs`, no fim:
```rust
#[cfg(test)]
mod tests {
    use super::clean_transcript;
    #[test]
    fn keeps_normal_speech() {
        assert_eq!(clean_transcript("  Let's review the timeline. ").as_deref(), Some("Let's review the timeline."));
    }
    #[test]
    fn drops_markers_and_hallucinations() {
        for raw in ["[BLANK_AUDIO]", " (music) ", "*laughs*", "Thanks for watching!", "[ Silence ]", ""] {
            assert_eq!(clean_transcript(raw), None, "{raw:?}");
        }
    }
    #[test]
    fn strips_markers_inside_text() {
        assert_eq!(clean_transcript("[MUSIC] Good morning everyone.").as_deref(), Some("Good morning everyone."));
    }
}
```

`crates/core/src/wav.rs`, no fim:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn reads_fixture_at_16k() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/s1.wav");
        let audio = super::read_wav_16k_mono(&p).unwrap();
        assert!(audio.len() > 16_000, "len = {}", audio.len());
        assert!(audio.iter().any(|s| s.abs() > 0.05));
    }
}
```

- [ ] **Step 4: Rodar e ver falhar**

Run: `cargo test -p gow-meet-core wer asr wav`
Expected: erro de compilação.

- [ ] **Step 5: Implementar WER**

Topo de `wer.rs`:
```rust
fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Word error rate: distância de edição entre palavras / palavras da referência.
pub fn wer(reference: &str, hypothesis: &str) -> f32 {
    let r = words(reference);
    let h = words(hypothesis);
    if r.is_empty() { return if h.is_empty() { 0.0 } else { 1.0 }; }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![i; h.len() + 1];
        for j in 1..=h.len() {
            let sub = prev[j - 1] + usize::from(r[i - 1] != h[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f32 / r.len() as f32
}
```

- [ ] **Step 6: Implementar leitura de WAV**

Topo de `wav.rs`:
```rust
use crate::resample::{to_mono, Resampler, SAMPLE_RATE};
use anyhow::Result;
use std::path::Path;

pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / max)).collect::<Result<_, _>>()?
        }
    };
    let mono = to_mono(&samples, spec.channels as usize);
    if spec.sample_rate == SAMPLE_RATE { return Ok(mono); }
    Ok(Resampler::new(spec.sample_rate, SAMPLE_RATE).process(&mono))
}
```

- [ ] **Step 7: Implementar `asr.rs`**

Topo de `asr.rs`:
```rust
use anyhow::{Context, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

pub trait Transcriber: Send {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String>;
}

const HALLUCINATIONS: &[&str] = &[
    "thanks for watching!", "thank you for watching.", "thank you for watching!",
    "subtitles by the amara.org community",
];

/// Remove marcadores como `[BLANK_AUDIO]`, `(music)`, `*laughs*` e frases que o
/// Whisper inventa em silêncio. `None` quando não sobra fala.
pub fn clean_transcript(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0i32;
    let mut in_star = false;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            '*' => in_star = !in_star,
            _ if depth == 0 && !in_star => out.push(c),
            _ => {}
        }
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() || HALLUCINATIONS.contains(&text.to_lowercase().as_str()) { return None; }
    Some(text)
}

pub struct WhisperTranscriber { _ctx: WhisperContext, state: WhisperState, lang: String }

impl WhisperTranscriber {
    pub fn new(model_path: &Path, lang: &str, use_gpu: bool) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().context("caminho do modelo não é UTF-8")?, params,
        ).context("carregar modelo Whisper")?;
        let state = ctx.create_state().context("criar estado Whisper")?;
        Ok(Self { _ctx: ctx, state, lang: lang.to_string() })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String> {
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some(&self.lang));
        p.set_no_context(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        self.state.full(p, audio).context("whisper full")?;
        let n = self.state.full_n_segments()?;
        let mut text = String::new();
        for i in 0..n {
            text.push_str(&self.state.full_get_segment_text(i)?);
        }
        Ok(text)
    }
}
```
Se a versão resolvida do `whisper-rs` mudou a assinatura de `full_n_segments`/`full_get_segment_text` (a 0.15 troca por iterador de segmentos), fixe `whisper-rs = "=0.14.x"` com a última 0.14 publicada em vez de reescrever.

- [ ] **Step 8: Rodar os testes unitários**

Run: `cargo test -p gow-meet-core wer asr wav`
Expected: 7 PASS. Primeira compilação do whisper.cpp leva alguns minutos.

- [ ] **Step 9: Teste de integração com modelo real (ignorado por padrão)**

`crates/core/tests/asr_whisper.rs`:
```rust
//! Rode com: GOW_MEET_WHISPER=models/tmp/ggml-small.en-q5_1.bin cargo test -p gow-meet-core --release -- --ignored
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
```
Run: `GOW_MEET_WHISPER=models/tmp/ggml-small.en-q5_1.bin cargo test -p gow-meet-core --release --test asr_whisper -- --ignored --nocapture`
Expected: 2 PASS; WER de cada frase impresso. Se `silence_yields_no_line` falhar, acrescente o texto bruto impresso a `HALLUCINATIONS` (minúsculo) e um caso equivalente em `drops_markers_and_hallucinations`.

- [ ] **Step 10: Commit**

```bash
git add crates/core scripts/gen-fixtures.sh
git commit -m "Adiciona transcrição com whisper.cpp, WER e fixtures sintéticas"
```

---
### Task 6: Tradução com Opus-MT/CTranslate2

**Files:**
- Create: `crates/core/src/translate.rs`, `crates/core/tests/translate_ct2.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod translate;`), `crates/core/Cargo.toml`

**Interfaces:**
- Produces:
  - `trait Translator: Send { fn translate(&mut self, text: &str) -> anyhow::Result<String>; }`
  - `prepare_source(prefix: Option<&str>, text: &str) -> String`
  - `Ct2Translator::new(model_dir: &Path, target_prefix: Option<String>) -> Result<Self>`

- [ ] **Step 1: Dependência**

`crates/core/Cargo.toml`:
```toml
[target.'cfg(target_os = "macos")'.dependencies]
ct2rs = { version = "0.9", features = ["accelerate"] }

[target.'cfg(not(target_os = "macos"))'.dependencies]
ct2rs = { version = "0.9" }
```
(Acrescente às seções `target` já existentes da Task 5, sem duplicá-las.) O backend de CPU do `ct2rs` no Windows é decidido no spike (Task 9); se o job Windows do CI quebrar por causa do `ct2rs` antes disso, registre o erro e resolva na Task 9, sem bloquear as tasks seguintes no Mac.

- [ ] **Step 2: Testes unitários**

Fim de `translate.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::prepare_source;
    #[test]
    fn adds_target_prefix_when_configured() {
        assert_eq!(prepare_source(Some(">>por<<"), " Hello team. "), ">>por<< Hello team.");
    }
    #[test]
    fn no_prefix_just_trims() {
        assert_eq!(prepare_source(None, " Hello team. "), "Hello team.");
    }
}
```

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo test -p gow-meet-core translate`
Expected: erro de compilação.

- [ ] **Step 4: Implementar**

Topo de `translate.rs`:
```rust
use anyhow::{Context, Result};
use std::path::Path;

pub trait Translator: Send {
    fn translate(&mut self, text: &str) -> Result<String>;
}

pub fn prepare_source(prefix: Option<&str>, text: &str) -> String {
    match prefix {
        Some(p) => format!("{p} {}", text.trim()),
        None => text.trim().to_string(),
    }
}

pub struct Ct2Translator { inner: ct2rs::Translator<ct2rs::tokenizers::auto::Tokenizer>, prefix: Option<String> }

impl Ct2Translator {
    pub fn new(model_dir: &Path, target_prefix: Option<String>) -> Result<Self> {
        let inner = ct2rs::Translator::new(model_dir, &ct2rs::Config::default())
            .context("carregar modelo de tradução")?;
        Ok(Self { inner, prefix: target_prefix })
    }
}

impl Translator for Ct2Translator {
    fn translate(&mut self, text: &str) -> Result<String> {
        let src = prepare_source(self.prefix.as_deref(), text);
        let out = self.inner
            .translate_batch(&[src], &ct2rs::TranslationOptions::default(), None)
            .context("traduzir")?;
        Ok(out.into_iter().next().map(|(t, _)| t.trim().to_string()).unwrap_or_default())
    }
}
```
Se o tipo genérico de `ct2rs::Translator` diferir na versão resolvida, use o tipo que `Translator::new` devolve segundo `docs.rs/ct2rs/<versão>` — a assinatura pública de `Ct2Translator` não muda.

- [ ] **Step 5: Rodar os testes unitários**

Run: `cargo test -p gow-meet-core translate`
Expected: 2 PASS.

- [ ] **Step 6: Teste de integração (ignorado por padrão)**

`crates/core/tests/translate_ct2.rs`:
```rust
//! GOW_MEET_OPUS=models/opus-mt-en-pt-ct2 GOW_MEET_OPUS_PREFIX='>>por<<' cargo test -p gow-meet-core --release --test translate_ct2 -- --ignored --nocapture
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
        let pt = t.translate(en).unwrap();
        eprintln!("{en} -> {pt}");
        assert!(pt.to_lowercase().contains(must), "{pt:?} não contém {must:?}");
    }
}
```
Run o comando do cabeçalho. Expected: PASS, traduções impressas. Se a saída vier em espanhol ou com o token de idioma, o prefixo está errado: confira o model card (Task 4, Step 2).

- [ ] **Step 7: Commit**

```bash
git add crates/core
git commit -m "Adiciona tradução local com Opus-MT via CTranslate2"
```

---
### Task 7: Pipeline — segmentação → transcrição → tradução → sessão

**Files:**
- Create: `crates/core/src/pipeline.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod pipeline;`)

**Interfaces:**
- Consumes: `Segmenter`, `SegmenterConfig`, `SegmentEvent`, `RmsDetector` (Task 3); `Transcriber`, `clean_transcript` (Task 5); `Translator` (Task 6); `Line`, `Speaker`, `SpeakerSource`, `SessionWriter`, `Meta`, `replay` (Task 2)
- Produces:
  - `enum PipelineEvent { Partial { id, speaker, t0, t1, text, translation: Option<String> }, Final(Line), Translated(Line), Lag { ms: u64 }, Error(String) }`
  - `trait Sink: Send + Sync { fn emit(&self, ev: PipelineEvent); }`
  - `PipelineConfig { segmenter, vad_threshold: f32, partial_translate_every_ms: u64, lag_warn_ms: u64 }` (com `Default`: padrão do segmentador, 0.01, 1500, 5000)
  - `Pipeline::start(cfg, Box<dyn Transcriber>, Box<dyn Translator>, SessionWriter, Arc<dyn Sink>) -> Pipeline`
  - `Pipeline::feed(&self, source: SpeakerSource, samples: &[f32])` — 16 kHz mono, chamável de várias threads
  - `Pipeline::finish(self, ended_ms: u64) -> Result<PathBuf>` — caminho do `transcript.md`
  - `line_id(SpeakerSource, seq: u64) -> String` (`"m3"` para microfone, `"s3"` para sistema)
  - `AsrJob { source, seq, start_ms, end_ms, audio, is_final }`, `coalesce(Vec<AsrJob>) -> Vec<AsrJob>`

- [ ] **Step 1: Testes**

Fim de `crates/core/src/pipeline.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{replay, Meta};

    const MS: usize = 16;
    fn tone(ms: usize) -> Vec<f32> { (0..ms * MS).map(|i| 0.3 * (i as f32 * 0.17).sin()).collect() }
    fn silence(ms: usize) -> Vec<f32> { vec![0.0; ms * MS] }

    struct FakeAsr(&'static str);
    impl Transcriber for FakeAsr {
        fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok(self.0.to_string()) }
    }
    struct Prefix;
    impl Translator for Prefix {
        fn translate(&mut self, t: &str) -> Result<String> { Ok(format!("pt: {t}")) }
    }
    struct Broken;
    impl Translator for Broken {
        fn translate(&mut self, _t: &str) -> Result<String> { anyhow::bail!("falhou") }
    }
    #[derive(Default)]
    struct Collect(Mutex<Vec<PipelineEvent>>);
    impl Sink for Collect {
        fn emit(&self, ev: PipelineEvent) { self.0.lock().unwrap().push(ev); }
    }

    fn run(asr: &'static str, tr: Box<dyn Translator>, feed: impl FnOnce(&Pipeline))
        -> (tempfile::TempDir, Vec<PipelineEvent>, Vec<Line>) {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let sink = Arc::new(Collect::default());
        let p = Pipeline::start(PipelineConfig::default(), Box::new(FakeAsr(asr)), tr, writer, sink.clone());
        feed(&p);
        let md = p.finish(10_000).unwrap();
        let lines = replay(&md.parent().unwrap().join("lines.jsonl")).unwrap();
        let events = sink.0.lock().unwrap().clone();
        (root, events, lines)
    }

    #[test]
    fn final_then_translation_with_same_id() {
        let (_d, ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat());
        });
        assert!(ev.iter().any(|e| matches!(e, PipelineEvent::Final(l) if l.id == "s0" && l.translation.is_none())));
        assert!(ev.iter().any(|e| matches!(e, PipelineEvent::Translated(l)
            if l.id == "s0" && l.translation.as_deref() == Some("pt: Hello team."))));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].translation.as_deref(), Some("pt: Hello team."));
        assert_eq!(lines[0].speaker.label, "Participante");
    }

    #[test]
    fn failed_translation_keeps_original() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Broken), |p| {
            p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat());
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].translation_error);
        assert_eq!(lines[0].text, "Hello team.");
    }

    #[test]
    fn simultaneous_sources_get_distinct_ids() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            let a = [tone(1_500), silence(1_000)].concat();
            for (m, s) in a.chunks(480).zip(a.chunks(480)) {
                p.feed(SpeakerSource::Mic, m);
                p.feed(SpeakerSource::System, s);
            }
        });
        let mut ids: Vec<_> = lines.iter().map(|l| (l.id.clone(), l.speaker.label.clone())).collect();
        ids.sort();
        assert_eq!(ids, vec![("m0".into(), "Você".into()), ("s0".into(), "Participante".into())]);
    }

    #[test]
    fn hallucination_produces_no_line() {
        let (_d, ev, lines) = run("[BLANK_AUDIO]", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &[tone(1_500), silence(1_000)].concat());
        });
        assert!(lines.is_empty());
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(_))));
    }

    #[test]
    fn long_speech_emits_partial_before_final() {
        let (_d, ev, _lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &[tone(3_500), silence(1_000)].concat());
        });
        let first_partial = ev.iter().position(|e| matches!(e, PipelineEvent::Partial { id, .. } if id == "s0"));
        let first_final = ev.iter().position(|e| matches!(e, PipelineEvent::Final(_)));
        assert!(first_partial.is_some() && first_partial < first_final);
    }

    fn job(source: SpeakerSource, seq: u64, is_final: bool, end_ms: u64) -> AsrJob {
        AsrJob { source, seq, start_ms: 0, end_ms, audio: vec![], is_final }
    }

    #[test]
    fn coalesce_keeps_finals_and_latest_partial_per_source() {
        use SpeakerSource::*;
        let out = coalesce(vec![
            job(System, 0, false, 1_000), job(System, 0, false, 2_000), job(System, 0, true, 2_500),
            job(System, 1, false, 3_000), job(System, 1, false, 4_000), job(Mic, 0, false, 1_000),
        ]);
        let keys: Vec<_> = out.iter().map(|j| (j.source, j.seq, j.is_final, j.end_ms)).collect();
        assert_eq!(keys, vec![(System, 0, true, 2_500), (System, 1, false, 4_000), (Mic, 0, false, 1_000)]);
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p gow-meet-core pipeline`
Expected: erro de compilação.

- [ ] **Step 3: Implementar**

Topo de `pipeline.rs`:
```rust
use crate::asr::{clean_transcript, Transcriber};
use crate::segmenter::{RmsDetector, SegmentEvent, Segmenter, SegmenterConfig};
use crate::session::{Line, SessionWriter, Speaker, SpeakerSource};
use crate::translate::Translator;
use anyhow::{Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineEvent {
    Partial { id: String, speaker: Speaker, t0: u64, t1: u64, text: String, translation: Option<String> },
    Final(Line),
    Translated(Line),
    Lag { ms: u64 },
    Error(String),
}

pub trait Sink: Send + Sync {
    fn emit(&self, ev: PipelineEvent);
}

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub segmenter: SegmenterConfig,
    pub vad_threshold: f32,
    pub partial_translate_every_ms: u64,
    pub lag_warn_ms: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { segmenter: SegmenterConfig::default(), vad_threshold: 0.01,
               partial_translate_every_ms: 1_500, lag_warn_ms: 5_000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsrJob {
    pub source: SpeakerSource,
    pub seq: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub audio: Vec<f32>,
    pub is_final: bool,
}

pub fn line_id(source: SpeakerSource, seq: u64) -> String {
    match source { SpeakerSource::Mic => format!("m{seq}"), SpeakerSource::System => format!("s{seq}") }
}

fn speaker_for(source: SpeakerSource) -> Speaker {
    match source { SpeakerSource::Mic => Speaker::mic(), SpeakerSource::System => Speaker::system() }
}

/// Sob atraso: mantém todos os finais e só a parcial mais recente de cada fonte,
/// descartando parciais cuja fala já fechou no mesmo lote.
pub fn coalesce(batch: Vec<AsrJob>) -> Vec<AsrJob> {
    let finals: HashSet<(SpeakerSource, u64)> =
        batch.iter().filter(|j| j.is_final).map(|j| (j.source, j.seq)).collect();
    let mut last: HashMap<SpeakerSource, usize> = HashMap::new();
    for (i, j) in batch.iter().enumerate() {
        if !j.is_final && !finals.contains(&(j.source, j.seq)) { last.insert(j.source, i); }
    }
    batch.into_iter().enumerate()
        .filter(|(i, j)| j.is_final || last.get(&j.source) == Some(i))
        .map(|(_, j)| j)
        .collect()
}

/// Gravação com nova tentativa: falas que falharam ficam em memória e são
/// regravadas antes da próxima.
struct Store { writer: SessionWriter, pending: Vec<Line> }

impl Store {
    fn append(&mut self, line: &Line, sink: &dyn Sink) {
        self.pending.push(line.clone());
        while let Some(l) = self.pending.first() {
            if let Err(e) = self.writer.append(l) {
                sink.emit(PipelineEvent::Error(format!("falha ao gravar transcrição: {e}")));
                return;
            }
            self.pending.remove(0);
        }
    }
}

enum TJob {
    Final(Line),
    Partial { id: String, speaker: Speaker, t0: u64, t1: u64, text: String },
}

pub struct Pipeline {
    cfg: PipelineConfig,
    segmenters: Mutex<HashMap<SpeakerSource, Segmenter>>,
    fed: Arc<Mutex<HashMap<SpeakerSource, u64>>>,
    asr_tx: Option<Sender<AsrJob>>,
    asr: Option<JoinHandle<()>>,
    tr: Option<JoinHandle<()>>,
    store: Arc<Mutex<Option<Store>>>,
}

impl Pipeline {
    pub fn start(
        cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>,
        writer: SessionWriter, sink: Arc<dyn Sink>,
    ) -> Self {
        let (asr_tx, asr_rx) = unbounded::<AsrJob>();
        let (tr_tx, tr_rx) = unbounded::<TJob>();
        let store = Arc::new(Mutex::new(Some(Store { writer, pending: vec![] })));
        let fed = Arc::new(Mutex::new(HashMap::new()));
        let asr_h = {
            let (store, sink, fed, warn) = (store.clone(), sink.clone(), fed.clone(), cfg.lag_warn_ms);
            std::thread::Builder::new().name("asr".into())
                .spawn(move || asr_loop(asr_rx, asr, tr_tx, store, sink, fed, warn)).expect("thread asr")
        };
        let tr_h = {
            let (store, every) = (store.clone(), Duration::from_millis(cfg.partial_translate_every_ms));
            std::thread::Builder::new().name("translate".into())
                .spawn(move || translate_loop(tr_rx, translator, store, sink, every)).expect("thread translate")
        };
        Self { cfg, segmenters: Mutex::new(HashMap::new()), fed, asr_tx: Some(asr_tx),
               asr: Some(asr_h), tr: Some(tr_h), store }
    }

    pub fn feed(&self, source: SpeakerSource, samples: &[f32]) {
        let events = {
            let mut segs = self.segmenters.lock().unwrap();
            let seg = segs.entry(source).or_insert_with(|| Segmenter::new(
                self.cfg.segmenter.clone(), Box::new(RmsDetector { threshold: self.cfg.vad_threshold })));
            seg.push(samples)
        };
        *self.fed.lock().unwrap().entry(source).or_insert(0) += samples.len() as u64;
        self.send(source, events);
    }

    fn send(&self, source: SpeakerSource, events: Vec<SegmentEvent>) {
        let Some(tx) = &self.asr_tx else { return };
        for ev in events {
            let job = match ev {
                SegmentEvent::Partial { seq, start_ms, audio } => {
                    let end_ms = start_ms + audio.len() as u64 / 16;
                    AsrJob { source, seq, start_ms, end_ms, audio, is_final: false }
                }
                SegmentEvent::Final { seq, start_ms, end_ms, audio } =>
                    AsrJob { source, seq, start_ms, end_ms, audio, is_final: true },
            };
            let _ = tx.send(job);
        }
    }

    pub fn finish(mut self, ended_ms: u64) -> Result<PathBuf> {
        let flushed: Vec<(SpeakerSource, Vec<SegmentEvent>)> = self.segmenters.lock().unwrap()
            .iter_mut().map(|(s, seg)| (*s, seg.flush())).collect();
        for (s, ev) in flushed { self.send(s, ev); }
        self.asr_tx.take();
        if let Some(h) = self.asr.take() { let _ = h.join(); }
        if let Some(h) = self.tr.take() { let _ = h.join(); }
        let store = self.store.lock().unwrap().take().context("sessão já encerrada")?;
        let mut writer = store.writer;
        for l in &store.pending { writer.append(l)?; }
        writer.finish(ended_ms)
    }
}

fn asr_loop(
    rx: Receiver<AsrJob>, mut asr: Box<dyn Transcriber>, tr_tx: Sender<TJob>,
    store: Arc<Mutex<Option<Store>>>, sink: Arc<dyn Sink>,
    fed: Arc<Mutex<HashMap<SpeakerSource, u64>>>, lag_warn_ms: u64,
) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        for job in coalesce(batch) {
            let fed_ms = fed.lock().unwrap().get(&job.source).copied().unwrap_or(0) / 16;
            let lag = fed_ms.saturating_sub(job.end_ms);
            if lag > lag_warn_ms { sink.emit(PipelineEvent::Lag { ms: lag }); }
            let raw = match asr.transcribe(&job.audio) {
                Ok(t) => t,
                Err(e) => { sink.emit(PipelineEvent::Error(format!("falha na transcrição: {e}"))); continue; }
            };
            let Some(text) = clean_transcript(&raw) else { continue };
            let (id, speaker) = (line_id(job.source, job.seq), speaker_for(job.source));
            if job.is_final {
                let line = Line { id, t0: job.start_ms, t1: job.end_ms, speaker, text,
                                  translation: None, translation_error: false };
                if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                sink.emit(PipelineEvent::Final(line.clone()));
                let _ = tr_tx.send(TJob::Final(line));
            } else {
                sink.emit(PipelineEvent::Partial { id: id.clone(), speaker: speaker.clone(),
                    t0: job.start_ms, t1: job.end_ms, text: text.clone(), translation: None });
                let _ = tr_tx.send(TJob::Partial { id, speaker, t0: job.start_ms, t1: job.end_ms, text });
            }
        }
    }
}

fn translate_loop(
    rx: Receiver<TJob>, mut tr: Box<dyn Translator>, store: Arc<Mutex<Option<Store>>>,
    sink: Arc<dyn Sink>, every: Duration,
) {
    let mut last_partial: HashMap<SpeakerSource, Instant> = HashMap::new();
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        let final_ids: HashSet<String> = batch.iter()
            .filter_map(|j| match j { TJob::Final(l) => Some(l.id.clone()), _ => None }).collect();
        let mut partials: HashMap<SpeakerSource, TJob> = HashMap::new();
        for job in batch {
            match job {
                TJob::Final(mut line) => {
                    match tr.translate(&line.text) {
                        Ok(t) => line.translation = Some(t),
                        Err(_) => line.translation_error = true,
                    }
                    if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                    sink.emit(PipelineEvent::Translated(line));
                }
                TJob::Partial { ref id, ref speaker, .. } if !final_ids.contains(id) => {
                    partials.insert(speaker.source, job);
                }
                TJob::Partial { .. } => {}
            }
        }
        for (src, job) in partials {
            if last_partial.get(&src).is_some_and(|t| t.elapsed() < every) { continue; }
            let TJob::Partial { id, speaker, t0, t1, text } = job else { continue };
            if let Ok(t) = tr.translate(&text) {
                last_partial.insert(src, Instant::now());
                sink.emit(PipelineEvent::Partial { id, speaker, t0, t1, text, translation: Some(t) });
            }
        }
    }
}
```

- [ ] **Step 4: Rodar os testes**

Run: `cargo test -p gow-meet-core pipeline`
Expected: 6 PASS. Rode também `cargo test -p gow-meet-core` inteiro para garantir que nada anterior quebrou.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "Adiciona pipeline de transcrição e tradução com descarte sob atraso"
```

---
### Task 8: CLI — tradução de um WAV no terminal, com métricas

Primeiro marco visível: depois desta task você já vê transcrição e tradução em ritmo de reunião no terminal, a partir de um arquivo.

**Files:**
- Modify: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`

**Interfaces:**
- Consumes: `Pipeline`, `PipelineConfig`, `PipelineEvent`, `Sink` (Task 7); `WhisperTranscriber` (Task 5); `Ct2Translator` (Task 6); `SessionWriter`, `Meta`, `session_id`, `SpeakerSource` (Task 2); `read_wav_16k_mono` (Task 5)
- Produces: binário `gow-meet-cli <wav> --whisper <arquivo> --opus <dir> [--prefix S] [--cpu] [--fast] [--repeat N] [--out DIR]`; features `vulkan` e `cuda` repassadas ao core.

- [ ] **Step 1: Dependências e features**

`crates/cli/Cargo.toml`:
```toml
[dependencies]
gow-meet-core = { path = "../core" }
anyhow.workspace = true
clap = { version = "4", features = ["derive"] }
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }

[features]
vulkan = ["gow-meet-core/vulkan"]
cuda = ["gow-meet-core/cuda"]
```

- [ ] **Step 2: Teste do cálculo de percentil**

No fim de `crates/cli/src/main.rs`:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn percentile_nearest_rank() {
        let v = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        assert_eq!(super::percentile(&v, 50.0), 500);
        assert_eq!(super::percentile(&v, 90.0), 900);
        assert_eq!(super::percentile(&[], 90.0), 0);
    }
}
```
Run: `cargo test -p gow-meet-cli` → Expected: falha de compilação.

- [ ] **Step 3: Implementar o CLI**

`crates/cli/src/main.rs` (acima do módulo de testes):
```rust
use anyhow::Result;
use clap::Parser;
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{session_id, Meta, SessionWriter, SpeakerSource};
use gow_meet_core::translate::Ct2Translator;
use gow_meet_core::wav::read_wav_16k_mono;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Roda o pipeline sobre um WAV, em ritmo real, e imprime latências.
#[derive(Parser)]
struct Args {
    wav: PathBuf,
    #[arg(long)] whisper: PathBuf,
    #[arg(long)] opus: PathBuf,
    #[arg(long)] prefix: Option<String>,
    /// Desliga a GPU do Whisper.
    #[arg(long)] cpu: bool,
    /// Alimenta o áudio sem esperar o tempo real (latência não é medida).
    #[arg(long)] fast: bool,
    /// Repete o áudio N vezes (ex.: 24 × 25 s ≈ 10 min).
    #[arg(long, default_value_t = 1)] repeat: usize,
    #[arg(long, default_value = "gow-meet-cli-out")] out: PathBuf,
}

struct Term { start: Instant, finals: Mutex<Vec<u64>>, partials: Mutex<Vec<u64>> }

impl Sink for Term {
    fn emit(&self, ev: PipelineEvent) {
        let now = self.start.elapsed().as_millis() as u64;
        match ev {
            PipelineEvent::Partial { t1, text, translation, .. } => {
                if translation.is_none() { self.partials.lock().unwrap().push(now.saturating_sub(t1)); }
                eprintln!("    … {}", translation.unwrap_or(text));
            }
            PipelineEvent::Final(l) => println!("[{}] {}", l.speaker.label, l.text),
            PipelineEvent::Translated(l) => {
                self.finals.lock().unwrap().push(now.saturating_sub(l.t1));
                println!("    → {}", l.translation.as_deref().unwrap_or("(sem tradução)"));
            }
            PipelineEvent::Lag { ms } => eprintln!("! atraso de {ms} ms"),
            PipelineEvent::Error(e) => eprintln!("! {e}"),
        }
    }
}

fn percentile(v: &[u64], p: f64) -> u64 {
    if v.is_empty() { return 0; }
    let mut s = v.to_vec();
    s.sort_unstable();
    let rank = ((p * s.len() as f64) / 100.0).ceil().max(1.0) as usize;
    s[rank - 1]
}

fn main() -> Result<()> {
    let a = Args::parse();
    let one = read_wav_16k_mono(&a.wav)?;
    let audio: Vec<f32> = std::iter::repeat(one).take(a.repeat).flatten().collect();
    let asr = WhisperTranscriber::new(&a.whisper, "en", !a.cpu)?;
    let tr = Ct2Translator::new(&a.opus, a.prefix.clone())?;
    let now = chrono::Local::now();
    let meta = Meta { v: 1, id: session_id(now), title: "CLI".into(), started: now.timestamp_millis() as u64,
        ended: None, source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
        app_version: gow_meet_core::version().into() };
    let writer = SessionWriter::create(&a.out, meta)?;
    let sink = Arc::new(Term { start: Instant::now(), finals: Mutex::new(vec![]), partials: Mutex::new(vec![]) });
    let p = Pipeline::start(PipelineConfig::default(), Box::new(asr), Box::new(tr), writer, sink.clone());
    for (i, chunk) in audio.chunks(1_600).enumerate() {
        if !a.fast {
            let due = sink.start + Duration::from_millis((i as u64 + 1) * 100);
            if let Some(wait) = due.checked_duration_since(Instant::now()) { std::thread::sleep(wait); }
        }
        p.feed(SpeakerSource::System, chunk);
    }
    let audio_ms = audio.len() as u64 / 16;
    let md = p.finish(audio_ms)?;
    let wall = sink.start.elapsed().as_millis() as u64;
    let f = sink.finals.lock().unwrap();
    let pa = sink.partials.lock().unwrap();
    eprintln!("\náudio: {audio_ms} ms | tempo total: {wall} ms | falas: {}", f.len());
    if !a.fast {
        eprintln!("fala fechada → tradução: p50 {} ms, p90 {} ms", percentile(&f, 50.0), percentile(&f, 90.0));
        eprintln!("parcial (transcrição): p50 {} ms, p90 {} ms", percentile(&pa, 50.0), percentile(&pa, 90.0));
    }
    eprintln!("transcrição: {}", md.display());
    Ok(())
}
```

- [ ] **Step 4: Testes e execução no Mac**

```bash
cargo test -p gow-meet-cli
cargo run -p gow-meet-cli --release -- crates/core/tests/fixtures/meeting.wav \
  --whisper models/tmp/ggml-small.en-q5_1.bin --opus models/opus-mt-en-pt-ct2 --prefix '>>por<<'
```
Expected: teste PASS; no terminal, em ritmo de fala, cada frase em inglês seguida de `→` com a tradução; ao final, p50/p90 e o caminho do `transcript.md`. Anote os números do Mac em `docs/spike-windows.md` como referência.

- [ ] **Step 5: Commit**

```bash
git add crates/cli
git commit -m "Adiciona CLI que traduz um WAV em ritmo real e mede latência"
```

---

### Task 9: Spike de viabilidade no Windows (RTX 2050)

Descartável: o código não muda; o resultado é o documento de decisão.

**Files:**
- Create: `docs/spike-windows.md`

- [ ] **Step 1: Preparar o notebook Windows**

Instale: Rust (rustup, toolchain MSVC), Visual Studio Build Tools com "Desktop development with C++", CMake, Git, e:
- para Vulkan: Vulkan SDK (LunarG);
- para CUDA: CUDA Toolkit 12.x compatível com o driver da RTX 2050.

Copie para a máquina, sem dados reais: o repositório, `models/tmp/*.bin` e `models/opus-mt-en-pt-ct2/`.

- [ ] **Step 2: Compilar as duas variantes**

```powershell
cargo build -p gow-meet-cli --release --features vulkan
copy target\release\gow-meet-cli.exe gow-meet-cli-vulkan.exe
cargo build -p gow-meet-cli --release --features cuda
copy target\release\gow-meet-cli.exe gow-meet-cli-cuda.exe
```
Se o `ct2rs` não compilar no Windows sem um backend de CPU, escolha um em `docs.rs/crate/ct2rs/<versão>/features` (registre qual) e acrescente-o na seção `cfg(not(target_os = "macos"))` do `crates/core/Cargo.toml`.

- [ ] **Step 3: Medir (10 min de áudio sintético, notebook na tomada, Teams aberto em segundo plano)**

Em um terminal, monitore:
```powershell
nvidia-smi --query-gpu=memory.used,utilization.gpu --format=csv -l 1 > gpu.csv
```
Em outro:
```powershell
typeperf "\Processor(_Total)\% Processor Time" -si 1 > cpu.csv
```
E rode cada combinação:
```powershell
.\gow-meet-cli-vulkan.exe crates\core\tests\fixtures\meeting.wav --repeat 24 --whisper models\tmp\ggml-small.en-q5_1.bin --opus models\opus-mt-en-pt-ct2 --prefix ">>por<<"
.\gow-meet-cli-vulkan.exe crates\core\tests\fixtures\meeting.wav --repeat 24 --whisper models\tmp\ggml-large-v3-turbo-q5_0.bin --opus models\opus-mt-en-pt-ct2 --prefix ">>por<<"
.\gow-meet-cli-cuda.exe   (mesmos dois comandos)
```

- [ ] **Step 4: Registrar e decidir**

`docs/spike-windows.md`:
```markdown
# Spike Windows — RTX 2050

Máquina: <modelo do notebook, CPU, RAM, driver NVIDIA> · Data: <data>

| Backend | Modelo | Fechada→tradução p50/p90 | Parcial p50/p90 | VRAM máx | CPU média | Observações |
|---|---|---|---|---|---|---|
| Vulkan | small.en q5_1 | | | | | |
| Vulkan | turbo q5_0 | | | | | |
| CUDA | small.en q5_1 | | | | | |
| CUDA | turbo q5_0 | | | | | |
| Mac M2 (referência) | small.en q5_1 | | | — | | |

Metas: fechada ≤ 3 s no p90; parcial ≤ 1,5 s; VRAM ≤ 3 GB; CPU ≤ 30%.

Decisão: backend <…>, modelo <…>, backend de CPU do ct2rs <…>. Motivo: <…>.
```
Preencha com os números medidos (os `<…>` são campos do relatório, não do código). Se nenhuma combinação atingir as metas, pare e discuta antes da Task 14: opções são modelo `base.en`, reduzir `partial_every_ms` ou desligar parciais traduzidas.

- [ ] **Step 5: Commit**

```bash
git add docs/spike-windows.md crates/core/Cargo.toml
git commit -m "Registra resultado do spike de viabilidade no Windows"
```

---
### Task 10: Captura no Mac — áudio do sistema (ScreenCaptureKit) e microfone (cpal)

**Files:**
- Create: `crates/core/src/capture/mod.rs`, `crates/core/src/capture/mic.rs`, `crates/core/src/capture/mac.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod capture;`), `crates/core/Cargo.toml`, `crates/cli/src/main.rs` (modo `--live`)

**Interfaces:**
- Consumes: `to_mono`, `Resampler`, `SAMPLE_RATE` (Task 3); `SpeakerSource` (Task 2)
- Produces:
  - `enum CaptureEvent { Audio { source: SpeakerSource, samples: Vec<f32> }, Reconnecting(SpeakerSource), Error { source: SpeakerSource, message: String } }` — áudio sempre 16 kHz mono
  - `trait AudioSource: Send { fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()>; fn stop(&mut self); }`
  - `Converter::new(in_rate: u32, channels: usize)`, `.process(&mut self, interleaved: &[f32]) -> Vec<f32>`
  - `MicSource::new()` (todas as plataformas); `SystemSource::new()` (Mac nesta task; Windows na Task 14)

- [ ] **Step 1: Dependências**

`crates/core/Cargo.toml`:
```toml
[dependencies]
cpal = "0.15"

[target.'cfg(target_os = "macos")'.dependencies]
screencapturekit = "0.3"
core-media-rs = "0.3"
```
(Junte às seções existentes.) Confira em `docs.rs/screencapturekit/<versão resolvida>` os caminhos de módulo usados no Step 6; se mudaram, ajuste só os `use`, mantendo a interface `SystemSource`.

- [ ] **Step 2: Testes do `Converter`**

`crates/core/src/capture/mod.rs`, no fim:
```rust
#[cfg(test)]
mod tests {
    use super::Converter;
    #[test]
    fn stereo_48k_becomes_mono_16k() {
        let mut c = Converter::new(48_000, 2);
        let stereo: Vec<f32> = (0..48_000).flat_map(|_| [0.5f32, 0.5]).collect();
        let out = c.process(&stereo);
        assert!((out.len() as i64 - 16_000).abs() <= 2);
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-3));
    }
    #[test]
    fn mono_16k_passes_through() {
        let mut c = Converter::new(16_000, 1);
        assert_eq!(c.process(&[0.1, 0.2, 0.3]), vec![0.1, 0.2, 0.3]);
    }
}
```
Run: `cargo test -p gow-meet-core capture` → Expected: falha de compilação.

- [ ] **Step 3: Implementar `capture/mod.rs`**

```rust
use crate::resample::{to_mono, Resampler, SAMPLE_RATE};
use crate::session::SpeakerSource;
use anyhow::Result;
use crossbeam_channel::Sender;

pub mod mic;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "macos")]
pub use mac::SystemSource;
#[cfg(not(target_os = "macos"))]
pub use unsupported::SystemSource;
pub use mic::MicSource;

/// Mantém o CI do Windows compilando até a Task 14 implementar o loopback WASAPI.
#[cfg(not(target_os = "macos"))]
mod unsupported {
    use super::{AudioSource, CaptureEvent};
    use anyhow::Result;
    use crossbeam_channel::Sender;
    #[derive(Default)]
    pub struct SystemSource;
    impl SystemSource { pub fn new() -> Self { Self } }
    impl AudioSource for SystemSource {
        fn start(&mut self, _tx: Sender<CaptureEvent>) -> Result<()> {
            anyhow::bail!("captura do áudio do sistema ainda não disponível nesta plataforma")
        }
        fn stop(&mut self) {}
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureEvent {
    Audio { source: SpeakerSource, samples: Vec<f32> },
    Reconnecting(SpeakerSource),
    Error { source: SpeakerSource, message: String },
}

pub trait AudioSource: Send {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()>;
    fn stop(&mut self);
}

pub struct Converter { channels: usize, resampler: Option<Resampler> }

impl Converter {
    pub fn new(in_rate: u32, channels: usize) -> Self {
        let resampler = (in_rate != SAMPLE_RATE).then(|| Resampler::new(in_rate, SAMPLE_RATE));
        Self { channels, resampler }
    }
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let mono = to_mono(interleaved, self.channels);
        match &mut self.resampler { Some(r) => r.process(&mono), None => mono }
    }
}
```

- [ ] **Step 4: Rodar os testes**

Run: `cargo test -p gow-meet-core capture` → Expected: 2 PASS (após Steps 5 e 6 compilarem; se preferir, crie `mic.rs`/`mac.rs` vazios com `pub struct MicSource;`/`pub struct SystemSource;` para rodar já).

- [ ] **Step 5: Microfone com reconexão (`capture/mic.rs`)**

O `cpal::Stream` não é `Send` no Mac: a stream vive numa thread própria, que a recria quando o dispositivo padrão muda ou a stream dá erro.
```rust
use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct MicSource { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl MicSource {
    pub fn new() -> Self { Self { stop: Arc::new(AtomicBool::new(false)), thread: None } }
}

impl Default for MicSource { fn default() -> Self { Self::new() } }

fn default_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| d.name().ok())
}

fn open(tx: Sender<CaptureEvent>, failed: Arc<AtomicBool>) -> Result<cpal::Stream> {
    let device = cpal::default_host().default_input_device().context("nenhum microfone")?;
    let cfg = device.default_input_config()?;
    let conv = Mutex::new(Converter::new(cfg.sample_rate().0, cfg.channels() as usize));
    let stream = device.build_input_stream(
        &cfg.config(),
        move |data: &[f32], _| {
            let samples = conv.lock().unwrap().process(data);
            let _ = tx.send(CaptureEvent::Audio { source: SpeakerSource::Mic, samples });
        },
        move |_err| failed.store(true, Ordering::SeqCst),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

impl AudioSource for MicSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        let stop = self.stop.clone();
        stop.store(false, Ordering::SeqCst);
        self.thread = Some(std::thread::Builder::new().name("mic".into()).spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let failed = Arc::new(AtomicBool::new(false));
                let name = default_name();
                let stream = match open(tx.clone(), failed.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(CaptureEvent::Error { source: SpeakerSource::Mic, message: e.to_string() });
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                };
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(500));
                    if failed.load(Ordering::SeqCst) || default_name() != name {
                        let _ = tx.send(CaptureEvent::Reconnecting(SpeakerSource::Mic));
                        break;
                    }
                }
                drop(stream);
            }
        })?);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() { let _ = h.join(); }
    }
}
```
A configuração padrão de entrada no Mac é `f32`. Se `build_input_stream` falhar com formato não suportado, trate `cfg.sample_format()` e converta `i16` para `f32` (`v as f32 / 32768.0`) antes do `Converter`.

- [ ] **Step 6: Áudio do sistema no Mac (`capture/mac.rs`)**

O ScreenCaptureKit captura a mistura de áudio do sistema independente da saída (fone ou alto-falante), então trocar o fone não interrompe a captura. O próprio processo é excluído para não capturar sons do app.
```rust
use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{Context, Result};
use core_media_rs::cm_sample_buffer::CMSampleBuffer;
use crossbeam_channel::Sender;
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::{
    configuration::SCStreamConfiguration, content_filter::SCContentFilter,
    output_trait::SCStreamOutputTrait, output_type::SCStreamOutputType, SCStream,
};
use std::sync::Mutex;

const RATE: u32 = 48_000;

struct Handler { tx: Sender<CaptureEvent>, conv: Mutex<Converter> }

impl SCStreamOutputTrait for Handler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio { return; }
        let Ok(list) = sample.get_audio_buffer_list() else { return };
        for buf in list.buffers() {
            let data: Vec<f32> = buf.data().chunks_exact(4)
                .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect();
            let samples = self.conv.lock().unwrap().process(&data);
            let _ = self.tx.send(CaptureEvent::Audio { source: SpeakerSource::System, samples });
        }
    }
}

pub struct SystemSource { stream: Option<SCStream> }

impl SystemSource {
    pub fn new() -> Self { Self { stream: None } }
}

impl Default for SystemSource { fn default() -> Self { Self::new() } }

// SCStream é usado só pela thread que chama start/stop (o comando do Tauri).
unsafe impl Send for SystemSource {}

impl AudioSource for SystemSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        let content = SCShareableContent::get().map_err(|e| anyhow::anyhow!(
            "sem permissão de Gravação de Tela e Áudio do Sistema ({e:?})"))?;
        let display = content.displays().into_iter().next().context("nenhuma tela encontrada")?;
        let filter = SCContentFilter::new().with_display_excluding_windows(&display, &[]);
        let config = SCStreamConfiguration::new()
            .set_captures_audio(true)?
            .set_sample_rate(RATE)?
            .set_channel_count(1)?
            .set_excludes_current_process_audio(true)?
            .set_width(2)?
            .set_height(2)?;
        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(Handler { tx, conv: Mutex::new(Converter::new(RATE, 1)) },
                                  SCStreamOutputType::Audio);
        stream.start_capture().map_err(|e| anyhow::anyhow!("iniciar captura: {e:?}"))?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(s) = self.stream.take() { let _ = s.stop_capture(); }
    }
}
```
Vídeo de 2×2 px minimiza o custo do vídeo que o ScreenCaptureKit exige. Se a versão resolvida nomear métodos de forma diferente (ex.: `get_audio_buffer_list`), siga o exemplo de áudio do repositório da crate na mesma versão.

- [ ] **Step 7: Modo `--live` no CLI**

Em `crates/cli/src/main.rs`, torne `wav` opcional (`wav: Option<PathBuf>`) e acrescente `#[arg(long)] live: bool`. No `main`, depois de criar `p`:
```rust
if a.live {
    use gow_meet_core::capture::{AudioSource, CaptureEvent, MicSource, SystemSource};
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut mic = MicSource::new();
    let mut sys = SystemSource::new();
    if let Err(e) = sys.start(tx.clone()) { eprintln!("! áudio do sistema: {e}"); }
    mic.start(tx)?;
    eprintln!("Capturando. Ctrl+C para encerrar.");
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    { let s = stop.clone(); ctrlc::set_handler(move || s.store(true, std::sync::atomic::Ordering::SeqCst))?; }
    let started = Instant::now();
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(CaptureEvent::Audio { source, samples }) => p.feed(source, &samples),
            Ok(CaptureEvent::Reconnecting(s)) => eprintln!("! reconectando {s:?}"),
            Ok(CaptureEvent::Error { source, message }) => eprintln!("! {source:?}: {message}"),
            Err(_) => {}
        }
    }
    mic.stop();
    sys.stop();
    let md = p.finish(started.elapsed().as_millis() as u64)?;
    eprintln!("transcrição: {}", md.display());
    return Ok(());
}
let wav = a.wav.clone().context("informe um WAV ou --live")?;
```
e troque `read_wav_16k_mono(&a.wav)` por `read_wav_16k_mono(&wav)` (carregue o WAV só depois deste bloco). Dependências do CLI: `crossbeam-channel.workspace = true`, `ctrlc = "3"`; `use anyhow::Context;`.

- [ ] **Step 8: Verificação manual da captura**

```bash
cargo run -p gow-meet-cli --release -- --live \
  --whisper models/tmp/ggml-small.en-q5_1.bin --opus models/opus-mt-en-pt-ct2 --prefix '>>por<<'
```
Na primeira vez o macOS pede permissão de microfone e de Gravação de Tela e Áudio do Sistema para o Terminal; conceda e rode de novo. Em outro terminal: `say -v Samantha "Let's review the project timeline for next week."`.
Expected: linha `[Participante] Let's review…` com tradução; falando no microfone, linhas `[Você] …`. Conecte/desconecte um fone durante a execução: a captura continua (no máximo um `! reconectando Mic`). Anote o resultado em `docs/validacao-mac.md` (criado na Task 13).

- [ ] **Step 9: Commit**

```bash
git add crates
git commit -m "Adiciona captura de áudio do sistema e microfone no Mac"
```

---
### Task 11: App Tauri — comandos, eventos, janelas, recuperação e logs

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`
- Create: `src-tauri/Info.plist`

**Interfaces:**
- Consumes: tudo do core (Tasks 2–10).
- Produces (contrato com o front-end, Task 12):
  - Comandos: `ensure_models() -> ()`, `start_session(title: Option<String>) -> String` (id da sessão), `stop_session() -> String` (caminho do `transcript.md`), `list_sessions() -> Vec<Meta>`
  - Evento `"gow-meet"` com payload `UiEvent` (JSON com campo `kind`):
    `{"kind":"partial","id","speaker","t0","text","translation"}`, `{"kind":"final","line"}`, `{"kind":"translated","line"}`, `{"kind":"lag","ms"}`, `{"kind":"error","message"}`, `{"kind":"reconnecting","source"}`, `{"kind":"stopped","transcript"}`
  - Evento `"models-progress"`: `{"file": String, "done": u64, "total": u64 | null}`
  - Janelas: `main` e `overlay` (`index.html#overlay`)

- [ ] **Step 1: Dependências**

`src-tauri/Cargo.toml`, `[dependencies]` (mantenha as geradas pelo `tauri init`):
```toml
gow-meet-core = { path = "../crates/core" }
serde = { workspace = true }
serde_json = { workspace = true }
crossbeam-channel = { workspace = true }
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
tracing = { workspace = true }
tracing-subscriber = "0.3"
tracing-appender = "0.2"
tauri-plugin-opener = "2"

[features]
vulkan = ["gow-meet-core/vulkan"]
cuda = ["gow-meet-core/cuda"]
```
`npm install @tauri-apps/plugin-opener@^2`.

- [ ] **Step 2: Teste do contrato de eventos**

No fim de `src-tauri/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::UiEvent;
    use gow_meet_core::session::{Line, Speaker};

    #[test]
    fn events_serialize_with_kind_tag() {
        let line = Line { id: "s0".into(), t0: 0, t1: 900, speaker: Speaker::system(),
            text: "Hello team.".into(), translation: Some("Olá, equipe.".into()), translation_error: false };
        let v = serde_json::to_value(UiEvent::Translated { line }).unwrap();
        assert_eq!(v["kind"], "translated");
        assert_eq!(v["line"]["speaker"]["label"], "Participante");
        let v = serde_json::to_value(UiEvent::Lag { ms: 6000 }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "lag", "ms": 6000}));
    }
}
```
Run: `cargo test -p gow-meet-app` → Expected: falha de compilação.

- [ ] **Step 3: Implementar `src-tauri/src/lib.rs`**

Substitua o conteúdo gerado (o `main.rs` gerado chama `gow_meet_app_lib::run()` ou similar; mantenha o nome da lib que o `tauri init` criou em `[lib] name`):
```rust
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::capture::{AudioSource, CaptureEvent, MicSource, SystemSource};
use gow_meet_core::models::{self, HttpFetch, DEFAULT_BASE_URL};
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{recover_unfinished, session_id, Line, Meta, SessionWriter, Speaker, SpeakerSource};
use gow_meet_core::translate::Ct2Translator;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Escolhido no spike (docs/spike-windows.md). Troque se a decisão for outra.
const WHISPER_MODEL: &str = "whisper-small-en";
const OPUS_MODEL: &str = "opus-mt-en-pt";
/// Prefixo de idioma confirmado na Task 6.
const OPUS_PREFIX: Option<&str> = Some(">>por<<");

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiEvent {
    Partial { id: String, speaker: Speaker, t0: u64, text: String, translation: Option<String> },
    Final { line: Line },
    Translated { line: Line },
    Lag { ms: u64 },
    Error { message: String },
    Reconnecting { source: SpeakerSource },
    Stopped { transcript: String },
}

struct TauriSink(AppHandle);

impl Sink for TauriSink {
    fn emit(&self, ev: PipelineEvent) {
        let ui = match ev {
            PipelineEvent::Partial { id, speaker, t0, text, translation, .. } =>
                UiEvent::Partial { id, speaker, t0, text, translation },
            PipelineEvent::Final(line) => UiEvent::Final { line },
            PipelineEvent::Translated(line) => UiEvent::Translated { line },
            PipelineEvent::Lag { ms } => UiEvent::Lag { ms },
            PipelineEvent::Error(message) => { tracing::error!("pipeline: {message}"); UiEvent::Error { message } }
        };
        let _ = self.0.emit("gow-meet", ui);
    }
}

struct Running {
    pipeline: Arc<Pipeline>,
    mic: MicSource,
    sys: SystemSource,
    stop: Arc<AtomicBool>,
    pump: JoinHandle<()>,
    started: Instant,
}

#[derive(Default)]
struct AppState(Mutex<Option<Running>>);

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().data_dir().map(|d| d.join("gow-meet")).map_err(|e| e.to_string())
}

fn ensure(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let spec = models::manifest().remove(id).ok_or(format!("modelo {id} fora do manifesto"))?;
    let dir = root(app)?.join("models").join(id);
    let app2 = app.clone();
    let file = id.to_string();
    // Endereço configurável para redes que bloqueiam o Hugging Face.
    let base = std::env::var("GOW_MEET_MODELS_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    models::ensure(&spec, &base, &dir, &HttpFetch, &mut |done, total| {
        let _ = app2.emit("models-progress", serde_json::json!({"file": file, "done": done, "total": total}));
    }).map_err(|e| format!("baixar modelo {id}: {e}"))?;
    Ok(dir.join(&spec.files[0].name))
}

#[tauri::command]
async fn ensure_models(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure(&app, WHISPER_MODEL)?;
        ensure(&app, OPUS_MODEL)?;
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_session(app: AppHandle, title: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, title))
        .await.map_err(|e| e.to_string())?
}

fn start_blocking(app: &AppHandle, title: Option<String>) -> Result<String, String> {
    let state = app.state::<AppState>();
    if state.0.lock().unwrap().is_some() { return Err("já existe uma reunião em andamento".into()); }
    let whisper = ensure(app, WHISPER_MODEL)?;
    let opus_dir = ensure(app, OPUS_MODEL)?.parent().unwrap().to_path_buf();
    let asr = match WhisperTranscriber::new(&whisper, "en", true) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("GPU indisponível para o Whisper: {e}");
            let _ = app.emit("gow-meet", UiEvent::Error {
                message: "GPU indisponível: usando o processador, a tradução ficará mais lenta.".into() });
            WhisperTranscriber::new(&whisper, "en", false).map_err(|e| e.to_string())?
        }
    };
    let tr = Ct2Translator::new(&opus_dir, OPUS_PREFIX.map(String::from)).map_err(|e| e.to_string())?;
    let now = chrono::Local::now();
    let id = session_id(now);
    let meta = Meta {
        v: 1, id: id.clone(),
        title: title.filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("Reunião {}", now.format("%d/%m/%Y %H:%M"))),
        started: now.timestamp_millis() as u64, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(),
        models: [("asr".to_string(), WHISPER_MODEL.to_string()), ("translate".to_string(), OPUS_MODEL.to_string())].into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    };
    let writer = SessionWriter::create(&root(app)?.join("sessions"), meta).map_err(|e| e.to_string())?;
    let pipeline = Arc::new(Pipeline::start(PipelineConfig::default(), Box::new(asr), Box::new(tr),
                                            writer, Arc::new(TauriSink(app.clone()))));
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut sys = SystemSource::new();
    if let Err(e) = sys.start(tx.clone()) {
        tracing::warn!("áudio do sistema indisponível: {e}");
        let _ = app.emit("gow-meet", UiEvent::Error { message: e.to_string() });
    }
    let mut mic = MicSource::new();
    mic.start(tx).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let pump = {
        let (stop, pipeline, app) = (stop.clone(), pipeline.clone(), app.clone());
        std::thread::Builder::new().name("pump".into()).spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(CaptureEvent::Audio { source, samples }) => pipeline.feed(source, &samples),
                    Ok(CaptureEvent::Reconnecting(source)) => { let _ = app.emit("gow-meet", UiEvent::Reconnecting { source }); }
                    Ok(CaptureEvent::Error { message, .. }) => { let _ = app.emit("gow-meet", UiEvent::Error { message }); }
                    Err(_) => {}
                }
            }
        }).map_err(|e| e.to_string())?
    };
    *state.0.lock().unwrap() = Some(Running { pipeline, mic, sys, stop, pump, started: Instant::now() });
    if let Some(w) = app.get_webview_window("overlay") { let _ = w.show(); }
    tracing::info!("sessão iniciada");
    Ok(id)
}

#[tauri::command]
async fn stop_session(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut run = state.0.lock().unwrap().take().ok_or("nenhuma reunião em andamento")?;
        run.mic.stop();
        run.sys.stop();
        run.stop.store(true, Ordering::SeqCst);
        let _ = run.pump.join();
        let pipeline = Arc::try_unwrap(run.pipeline).map_err(|_| "pipeline ainda em uso".to_string())?;
        let md = pipeline.finish(run.started.elapsed().as_millis() as u64).map_err(|e| e.to_string())?;
        let transcript = md.display().to_string();
        let _ = app.emit("gow-meet", UiEvent::Stopped { transcript: transcript.clone() });
        if let Some(w) = app.get_webview_window("overlay") { let _ = w.hide(); }
        tracing::info!("sessão encerrada");
        Ok(transcript)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_sessions(app: AppHandle) -> Result<Vec<Meta>, String> {
    let dir = root(&app)?.join("sessions");
    let mut out: Vec<Meta> = std::fs::read_dir(&dir).into_iter().flatten().flatten()
        .filter_map(|e| std::fs::read(e.path().join("meta.json")).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    out.sort_by(|a, b| b.started.cmp(&a.started));
    Ok(out)
}

fn init_logs(root: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY).filename_prefix("gow-meet").filename_suffix("log")
        .max_log_files(7).build(root.join("logs")).ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt().with_writer(writer).with_ansi(false).init();
    Some(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            let root = root(app.handle())?;
            if let Some(guard) = init_logs(&root) { app.manage(guard); }
            let now = chrono::Local::now().timestamp_millis() as u64;
            match recover_unfinished(&root.join("sessions"), now) {
                Ok(done) if !done.is_empty() => tracing::info!("{} sessão(ões) recuperada(s)", done.len()),
                Ok(_) => {}
                Err(e) => tracing::error!("recuperação: {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ensure_models, start_session, stop_session, list_sessions])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o gow-meet");
}
```
Logs registram só eventos e erros; nenhuma chamada de `tracing` recebe texto de fala. Mantenha assim.

- [ ] **Step 4: Janelas e permissões do Tauri**

Em `src-tauri/tauri.conf.json`, ajuste `identifier` para `"com.gow.meet"`, `productName` para `"gow-meet"` e substitua `app`:
```json
"app": {
  "macOSPrivateApi": true,
  "windows": [
    { "label": "main", "title": "gow-meet", "url": "index.html", "width": 900, "height": 640 },
    { "label": "overlay", "title": "gow-meet legendas", "url": "index.html#overlay",
      "width": 440, "height": 320, "x": 40, "y": 80, "transparent": true, "decorations": false,
      "alwaysOnTop": true, "visibleOnAllWorkspaces": true, "skipTaskbar": true,
      "resizable": true, "shadow": false, "visible": false }
  ],
  "security": { "csp": null }
}
```
`src-tauri/capabilities/default.json`: inclua `"windows": ["main", "overlay"]` e, em `permissions`, `"core:default"`, `"core:window:allow-start-dragging"`, `"opener:default"`, `"opener:allow-open-path"` com escopo para o diretório de dados:
```json
{ "identifier": "opener:allow-open-path", "allow": [{ "path": "$DATA/gow-meet/**" }] }
```
`src-tauri/Info.plist`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>NSMicrophoneUsageDescription</key>
  <string>O gow-meet transcreve sua voz durante a reunião, localmente.</string>
</dict>
</plist>
```

- [ ] **Step 5: Rodar o teste e compilar**

```bash
cargo test -p gow-meet-app
cargo build -p gow-meet-app
```
Expected: teste PASS; build sem erros.

- [ ] **Step 6: Commit**

```bash
git add src-tauri package.json package-lock.json
git commit -m "Adiciona comandos, eventos, janelas, recuperação e logs no app Tauri"
```

---
### Task 12: Interface — janela flutuante e janela principal

**Files:**
- Create: `src/lib/transcript.ts`, `src/lib/transcript.test.ts`, `src/lib/useGowMeet.ts`, `src/Overlay.tsx`, `src/MainWindow.tsx`, `src/styles.css`
- Modify: `src/main.tsx`, `package.json`, `index.html`
- Delete: `src/App.tsx`, `src/App.css`, `src/index.css`, `src/assets/` (template do Vite)

**Interfaces:**
- Consumes: comandos e eventos da Task 11 (`ensure_models`, `start_session`, `stop_session`, `list_sessions`; eventos `"gow-meet"` e `"models-progress"`).
- Produces: `applyEvent(state, ev) -> State`, `lastEntries(state, n) -> Entry[]` (puras, testadas).

- [ ] **Step 1: Vitest**

```bash
npm install -D vitest@^2
npm pkg set scripts.test="vitest run"
```

- [ ] **Step 2: Testes do estado da transcrição**

`src/lib/transcript.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { applyEvent, initial, lastEntries, type Line, type UiEvent } from "./transcript";

const sys = { source: "system" as const, label: "Participante" };
const line = (id: string, t0: number, text: string, translation: string | null, err = false): Line =>
  ({ id, t0, t1: t0 + 900, speaker: sys, text, translation, translation_error: err });
const run = (...evs: UiEvent[]) => evs.reduce(applyEvent, initial);

describe("applyEvent", () => {
  it("partial → final → translated mantém uma entrada pelo id", () => {
    const s = run(
      { kind: "partial", id: "s0", speaker: sys, t0: 0, text: "Hello", translation: null },
      { kind: "final", line: line("s0", 0, "Hello team.", null) },
      { kind: "translated", line: line("s0", 0, "Hello team.", "Olá, equipe.") },
    );
    expect(s.order).toEqual(["s0"]);
    expect(s.byId.s0).toMatchObject({ final: true, text: "Hello team.", translation: "Olá, equipe." });
  });

  it("parcial atrasada não reabre fala fechada", () => {
    const s = run(
      { kind: "final", line: line("s0", 0, "Hello team.", null) },
      { kind: "partial", id: "s0", speaker: sys, t0: 0, text: "Hel", translation: null },
    );
    expect(s.byId.s0).toMatchObject({ final: true, text: "Hello team." });
  });

  it("falha de tradução fica marcada", () => {
    const s = run({ kind: "translated", line: line("s0", 0, "Hello team.", null, true) });
    expect(s.byId.s0.translationError).toBe(true);
  });

  it("ordena por t0 e devolve as últimas n", () => {
    const s = run(
      { kind: "final", line: line("s1", 2000, "B", null) },
      { kind: "final", line: line("m0", 1000, "A", null) },
      { kind: "final", line: line("s2", 3000, "C", null) },
      { kind: "final", line: line("s3", 4000, "D", null) },
    );
    expect(lastEntries(s, 3).map((e) => e.id)).toEqual(["s1", "s2", "s3"]);
  });

  it("avisos de atraso e reconexão somem na próxima fala fechada", () => {
    let s = run({ kind: "lag", ms: 6000 }, { kind: "reconnecting", source: "mic" });
    expect(s.lagMs).toBe(6000);
    expect(s.notice).toBe("Reconectando áudio…");
    s = applyEvent(s, { kind: "final", line: line("s0", 0, "Hi.", null) });
    expect(s.lagMs).toBeNull();
    expect(s.notice).toBeNull();
  });

  it("stopped guarda o caminho da transcrição", () => {
    expect(run({ kind: "stopped", transcript: "/tmp/x/transcript.md" }).transcript).toBe("/tmp/x/transcript.md");
  });
});
```
Run: `npm test` → Expected: falha (módulo inexistente).

- [ ] **Step 3: Implementar `src/lib/transcript.ts`**

```ts
export type Speaker = { source: "mic" | "system"; label: string };
export type Line = {
  id: string; t0: number; t1: number; speaker: Speaker;
  text: string; translation: string | null; translation_error?: boolean;
};
export type UiEvent =
  | { kind: "partial"; id: string; speaker: Speaker; t0: number; text: string; translation: string | null }
  | { kind: "final"; line: Line }
  | { kind: "translated"; line: Line }
  | { kind: "lag"; ms: number }
  | { kind: "error"; message: string }
  | { kind: "reconnecting"; source: "mic" | "system" }
  | { kind: "stopped"; transcript: string };

export type Entry = {
  id: string; speaker: Speaker; t0: number; text: string;
  translation: string | null; final: boolean; translationError: boolean;
};
export type State = {
  order: string[]; byId: Record<string, Entry>;
  lagMs: number | null; notice: string | null; transcript: string | null;
};

export const initial: State = { order: [], byId: {}, lagMs: null, notice: null, transcript: null };

function upsert(s: State, e: Entry): State {
  const byId = { ...s.byId, [e.id]: e };
  const order = s.byId[e.id] ? s.order : [...s.order, e.id].sort((a, b) => byId[a].t0 - byId[b].t0);
  return { ...s, byId, order };
}

const fromLine = (l: Line, prev?: Entry): Entry => ({
  id: l.id, speaker: l.speaker, t0: l.t0, text: l.text, final: true,
  translation: l.translation ?? prev?.translation ?? null,
  translationError: !!l.translation_error,
});

export function applyEvent(s: State, ev: UiEvent): State {
  switch (ev.kind) {
    case "partial": {
      const prev = s.byId[ev.id];
      if (prev?.final) return s;
      return upsert(s, {
        id: ev.id, speaker: ev.speaker, t0: ev.t0, text: ev.text, final: false,
        translation: ev.translation ?? prev?.translation ?? null, translationError: false,
      });
    }
    case "final":
      return { ...upsert(s, fromLine(ev.line, s.byId[ev.line.id])), lagMs: null, notice: null };
    case "translated":
      return upsert(s, { ...fromLine(ev.line, s.byId[ev.line.id]), translation: ev.line.translation });
    case "lag":
      return { ...s, lagMs: ev.ms };
    case "error":
      return { ...s, notice: ev.message };
    case "reconnecting":
      return { ...s, notice: "Reconectando áudio…" };
    case "stopped":
      return { ...s, transcript: ev.transcript };
  }
}

export function lastEntries(s: State, n = 3): Entry[] {
  return s.order.slice(-n).map((id) => s.byId[id]);
}
```
Run: `npm test` → Expected: 6 PASS.

- [ ] **Step 4: Hook de eventos `src/lib/useGowMeet.ts`**

```ts
import { listen } from "@tauri-apps/api/event";
import { useEffect, useReducer } from "react";
import { applyEvent, initial, type UiEvent } from "./transcript";

export function useGowMeet() {
  const [state, dispatch] = useReducer(applyEvent, initial);
  useEffect(() => {
    const un = listen<UiEvent>("gow-meet", (e) => dispatch(e.payload));
    return () => { un.then((f) => f()); };
  }, []);
  return state;
}
```

- [ ] **Step 5: Janela flutuante `src/Overlay.tsx`**

```tsx
import { lastEntries } from "./lib/transcript";
import { useGowMeet } from "./lib/useGowMeet";

export function Overlay() {
  const state = useGowMeet();
  const entries = lastEntries(state, 3);
  return (
    <div className="overlay" data-tauri-drag-region>
      {entries.length === 0 && <p className="hint" data-tauri-drag-region>Aguardando fala…</p>}
      {entries.map((e) => (
        <div key={e.id} className={`bubble ${e.final ? "" : "partial"}`} data-tauri-drag-region>
          <div className="speaker">{e.speaker.label}</div>
          <div className="translation">
            {e.translation ?? (e.translationError ? "(sem tradução)" : "…")}
          </div>
          <div className="original">{e.text}</div>
        </div>
      ))}
      {(state.lagMs || state.notice) && (
        <div className="status">
          {state.lagMs ? `Atraso de ${Math.round(state.lagMs / 1000)} s` : state.notice}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 6: Janela principal `src/MainWindow.tsx`**

```tsx
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { useGowMeet } from "./lib/useGowMeet";

type Meta = { id: string; title: string; started: number; ended: number | null };
type Progress = { file: string; done: number; total: number | null };

export function MainWindow() {
  const state = useGowMeet();
  const [title, setTitle] = useState("");
  const [running, setRunning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [sessions, setSessions] = useState<Meta[]>([]);

  const refresh = () => invoke<Meta[]>("list_sessions").then(setSessions).catch(() => {});
  useEffect(() => {
    refresh();
    const un = listen<Progress>("models-progress", (e) => setProgress(e.payload));
    return () => { un.then((f) => f()); };
  }, []);

  async function act(fn: () => Promise<void>) {
    setBusy(true); setError(null);
    try { await fn(); } catch (e) { setError(String(e)); } finally { setBusy(false); setProgress(null); }
  }

  const start = () => act(async () => {
    await invoke("ensure_models");
    await invoke("start_session", { title: title || null });
    setRunning(true);
  });
  const stop = () => act(async () => {
    await invoke("stop_session");
    setRunning(false);
    refresh();
  });

  return (
    <main className="main">
      <h1>gow-meet</h1>
      <p className="consent">Antes de começar, avise os participantes de que a reunião será transcrita.</p>
      {!running ? (
        <div className="row">
          <input placeholder="Título da reunião (opcional)" value={title} onChange={(e) => setTitle(e.target.value)} />
          <button disabled={busy} onClick={start}>{busy ? "Preparando…" : "Iniciar"}</button>
        </div>
      ) : (
        <button disabled={busy} onClick={stop}>{busy ? "Encerrando…" : "Encerrar"}</button>
      )}
      {progress && (
        <p>Baixando {progress.file}: {Math.round(progress.done / 1e6)} MB
          {progress.total ? ` de ${Math.round(progress.total / 1e6)} MB` : ""}</p>
      )}
      {error && <p className="error">{error}</p>}
      {state.notice && <p className="error">{state.notice}</p>}
      {state.notice?.includes("permissão") && (
        <p>Abra Ajustes do Sistema → Privacidade e Segurança → Gravação de Tela e Áudio do Sistema e Microfone,
          habilite o gow-meet e inicie de novo.</p>
      )}
      {state.transcript && (
        <button onClick={() => openPath(state.transcript!)}>Abrir última transcrição</button>
      )}
      <h2>Reuniões</h2>
      <ul>
        {sessions.map((s) => (
          <li key={s.id}>{s.title} — {new Date(s.started).toLocaleString("pt-BR")}{s.ended ? "" : " (em andamento)"}</li>
        ))}
      </ul>
    </main>
  );
}
```

- [ ] **Step 7: Roteamento, estilos e limpeza do template**

`src/main.tsx`:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import { MainWindow } from "./MainWindow";
import { Overlay } from "./Overlay";
import "./styles.css";

const isOverlay = window.location.hash === "#overlay";
document.documentElement.classList.toggle("is-overlay", isOverlay);
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isOverlay ? <Overlay /> : <MainWindow />}</React.StrictMode>,
);
```
`src/styles.css`:
```css
:root { font-family: -apple-system, "Segoe UI", system-ui, sans-serif; color-scheme: light dark; }
body { margin: 0; }
.is-overlay, .is-overlay body { background: transparent; }

.main { padding: 24px; max-width: 720px; }
.row { display: flex; gap: 8px; }
.row input { flex: 1; padding: 8px; }
button { padding: 8px 16px; cursor: pointer; }
.consent { opacity: 0.8; }
.error { color: #d33; }

.overlay {
  box-sizing: border-box; height: 100vh; padding: 14px; overflow: hidden;
  display: flex; flex-direction: column; justify-content: flex-end; gap: 8px;
  background: rgba(30, 30, 32, 0.88); border-radius: 22px; color: #fff; user-select: none;
}
.hint { color: #aaa; text-align: center; }
.bubble { background: rgba(255, 255, 255, 0.10); border-radius: 14px; padding: 10px 12px; }
.bubble.partial { opacity: 0.6; }
.speaker { font-size: 11px; color: #bbb; margin-bottom: 2px; }
.translation { font-size: 17px; line-height: 1.35; }
.original { font-size: 12px; color: #aaa; margin-top: 4px; }
.status { font-size: 11px; color: #f5c451; text-align: center; }
```
Remova `src/App.tsx`, `src/App.css`, `src/index.css`, `src/assets/`. Em `index.html`, troque o `<title>` para `gow-meet`.

- [ ] **Step 8: Verificar**

```bash
npm test
npm run build
npx tauri dev
```
Expected: testes PASS; build OK; abre a janela principal. Clique **Iniciar** (na primeira vez baixa os modelos, com progresso), conceda as permissões e rode `say -v Samantha "The new dashboard should be ready by Friday."` em outro terminal: a janela flutuante aparece com a tradução em destaque e o original embaixo. **Encerrar** esconde a janela flutuante e mostra "Abrir última transcrição".

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "Adiciona janela flutuante e janela principal"
```

---
### Task 13: Validação manual no Mac (reunião real pelo app do Teams)

**Files:**
- Create: `docs/validacao-mac.md`

- [ ] **Step 1: Build assinado com identidade fixa (para o macOS lembrar as permissões)**

No Acesso às Chaves: Assistente de Certificado → Criar um Certificado → nome `gow-meet dev`, tipo **Assinatura de Código**. Depois:
```bash
APPLE_SIGNING_IDENTITY="gow-meet dev" npx tauri build --bundles app
open target/release/bundle/macos/gow-meet.app
```
Expected: o app abre; as permissões concedidas continuam valendo depois de um novo build com a mesma identidade.

- [ ] **Step 2: Executar o roteiro e registrar**

`docs/validacao-mac.md` — registre só resultados e números, **nunca** nomes, falas ou trechos da reunião:
```markdown
# Validação manual — Mac

Máquina: Mac M2 8 GB · macOS: <versão> · Build: <commit> · Data: <data>

| # | Cenário | Resultado | Observação |
|---|---|---|---|
| 1 | Primeira abertura: baixa modelos com progresso e verifica hash | | |
| 2 | Permissões negadas: mensagem e orientação aparecem; com só microfone o app funciona | | |
| 3 | Reunião real em inglês pelo app do Teams: tradução acompanha a conversa | | atraso percebido: |
| 4 | Sua fala aparece como "Você"; a dos outros como "Participante" | | |
| 5 | Trocar fone ↔ alto-falante no meio: captura continua | | |
| 6 | Janela flutuante fica por cima do Teams (fora de tela cheia) e pode ser arrastada | | |
| 7 | Encerrar: `transcript.md` confere com a reunião (original + tradução) | | |
| 8 | Forçar saída (`kill -9`) no meio e reabrir: sessão é encerrada e `transcript.md` gerado | | |
| 9 | CPU com app aberto e parado (Monitor de Atividade) | | % |
| 10 | CPU e memória durante a reunião | | % / GB |
| 11 | Logs em `~/Library/Application Support/gow-meet/logs/` não contêm texto de fala | | |

Pendências encontradas: <lista ou "nenhuma">
```
Cada problema encontrado vira um teste que reproduz o caso (quando possível sem hardware) e uma correção em commit próprio, antes de seguir.

- [ ] **Step 3: Commit**

```bash
git add docs/validacao-mac.md
git commit -m "Registra validação manual no Mac"
```

---

### Task 14: Captura no Windows (loopback WASAPI) e validação no notebook do time

**Files:**
- Create: `crates/core/src/capture/watch.rs`, `crates/core/src/capture/windows.rs`, `docs/validacao-windows.md`
- Modify: `crates/core/src/capture/mod.rs`, `crates/core/src/capture/mic.rs`

**Interfaces:**
- Consumes: `AudioSource`, `CaptureEvent`, `Converter` (Task 10)
- Produces: `SystemSource` para Windows com a mesma interface do Mac; `spawn_watched(...)` compartilhado entre microfone e loopback.

- [ ] **Step 1: Extrair o laço de reconexão para `capture/watch.rs`**

```rust
use super::CaptureEvent;
use crate::session::SpeakerSource;
use anyhow::Result;
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub type OpenFn = fn(Sender<CaptureEvent>, Arc<AtomicBool>) -> Result<cpal::Stream>;

/// Mantém uma stream cpal viva numa thread própria; recria quando o dispositivo
/// padrão muda ou a stream reporta erro.
pub fn spawn_watched(
    source: SpeakerSource, stop: Arc<AtomicBool>, tx: Sender<CaptureEvent>,
    default_name: fn() -> Option<String>, open: OpenFn,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new().name(format!("capture-{source:?}")).spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let failed = Arc::new(AtomicBool::new(false));
            let name = default_name();
            let stream = match open(tx.clone(), failed.clone()) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(CaptureEvent::Error { source, message: e.to_string() });
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
                if failed.load(Ordering::SeqCst) || default_name() != name {
                    let _ = tx.send(CaptureEvent::Reconnecting(source));
                    break;
                }
            }
            drop(stream);
        }
    })
}
```
Em `mic.rs`, troque o corpo da thread em `start` por:
```rust
self.thread = Some(super::watch::spawn_watched(SpeakerSource::Mic, stop, tx, default_name, open)?);
```
(As funções `default_name` e `open` de `mic.rs` já têm as assinaturas esperadas.) Rode `cargo test -p gow-meet-core` e repita o Step 8 da Task 10 no Mac: o comportamento não pode mudar.

- [ ] **Step 2: Loopback em `capture/windows.rs`**

```rust
use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

fn default_name() -> Option<String> {
    cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}

/// No WASAPI, abrir uma stream de *entrada* num dispositivo de *saída* ativa o loopback.
fn open(tx: Sender<CaptureEvent>, failed: Arc<AtomicBool>) -> Result<cpal::Stream> {
    let device = cpal::default_host().default_output_device().context("nenhuma saída de áudio")?;
    let cfg = device.default_output_config()?;
    let conv = Mutex::new(Converter::new(cfg.sample_rate().0, cfg.channels() as usize));
    let stream = device.build_input_stream(
        &cfg.config(),
        move |data: &[f32], _| {
            let samples = conv.lock().unwrap().process(data);
            let _ = tx.send(CaptureEvent::Audio { source: SpeakerSource::System, samples });
        },
        move |_err| failed.store(true, Ordering::SeqCst),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

pub struct SystemSource { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl SystemSource {
    pub fn new() -> Self { Self { stop: Arc::new(AtomicBool::new(false)), thread: None } }
}

impl Default for SystemSource { fn default() -> Self { Self::new() } }

impl AudioSource for SystemSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        self.stop.store(false, Ordering::SeqCst);
        self.thread = Some(super::watch::spawn_watched(
            SpeakerSource::System, self.stop.clone(), tx, default_name, open)?);
        Ok(())
    }
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() { let _ = h.join(); }
    }
}
```
Trocar de fone no Windows muda o dispositivo de saída padrão: o laço detecta e reabre o loopback no novo dispositivo.

- [ ] **Step 3: Ligar no `capture/mod.rs`**

```rust
pub mod mic;
mod watch;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")]
pub use mac::SystemSource;
#[cfg(target_os = "windows")]
pub use windows::SystemSource;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub use unsupported::SystemSource;
```
e troque o `#[cfg(not(target_os = "macos"))]` do módulo `unsupported` por `#[cfg(not(any(target_os = "macos", target_os = "windows")))]`.

- [ ] **Step 4: CI e commit**

```bash
cargo test -p gow-meet-core
git add crates/core
git commit -m "Adiciona loopback WASAPI no Windows e unifica reconexão de captura"
git push   # se o remoto já existir; confira o job windows-latest verde
```

- [ ] **Step 5: Build e validação no notebook Windows**

No notebook (preparado na Task 9), com a feature decidida no spike:
```powershell
npm ci
npx tauri build --features vulkan   # ou cuda, conforme docs/spike-windows.md
```
Instale pelo instalador gerado em `target\release\bundle\` (sem assinatura, o SmartScreen avisa: "Mais informações → Executar assim mesmo"). Repita o roteiro da Task 13 numa reunião real pelo app do Teams e registre em `docs/validacao-windows.md`, com a mesma tabela (logs em `%APPDATA%\gow-meet\logs\`; CPU/GPU pelo Gerenciador de Tarefas). Mesmas regras: nada de nomes ou falas no documento.

- [ ] **Step 6: Commit**

```bash
git add docs/validacao-windows.md
git commit -m "Registra validação manual no Windows"
```

---

## Marcos visíveis

| Depois da task | O que você já vê |
|---|---|
| 8 | Tradução de um arquivo WAV no terminal, em ritmo de reunião |
| 10 | **Tradução simultânea de uma reunião real no terminal** (`gow-meet-cli --live`), no Mac |
| 12 | **Janela flutuante com a tradução**, no Mac — a primeira versão de uso |
| 13 | Primeira versão validada numa reunião real no Mac |
| 14 | Mesma versão funcionando nos notebooks Windows do time |
