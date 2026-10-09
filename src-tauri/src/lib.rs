mod config;

use config::Config;
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::capture::{AudioSource, CaptureEvent, MicSource, SystemSource};
use gow_meet_core::models::{self, HttpFetch};
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{recover_unfinished, session_id, Line, Meta, SessionWriter, Speaker, SpeakerSource};
use gow_meet_core::translate::Ct2Translator;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Contrato com o front-end: evento `"gow-meet"`, campo `kind` em camelCase.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiEvent {
    Partial { id: String, speaker: Speaker, t0: u64, text: String, translation: Option<String> },
    Final { line: Line },
    Translated { line: Line },
    Dropped { id: String },
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
            PipelineEvent::Dropped { id } => UiEvent::Dropped { id },
            PipelineEvent::Lag { ms } => UiEvent::Lag { ms },
            PipelineEvent::Error(message) => {
                // As mensagens do pipeline descrevem a falha, nunca o conteúdo da fala.
                tracing::error!("pipeline: {message}");
                UiEvent::Error { message }
            }
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
    started_ms: u64,
}

enum Slot {
    Idle,
    Starting,
    Running(Box<Running>),
    Stopping,
}

#[derive(Clone)]
struct ModelPaths {
    whisper: PathBuf,
    opus_dir: PathBuf,
}

struct AppState {
    config: Config,
    slot: Mutex<Slot>,
    /// Modelos já verificados (hash) nesta execução do app.
    models: Mutex<Option<ModelPaths>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().data_dir().map(|d| d.join("gow-meet")).map_err(|e| e.to_string())
}

/// Limita os eventos de progresso a ~4 por segundo; sempre emite o primeiro,
/// o início de um novo arquivo (done volta a baixar) e o 100%.
struct Throttle {
    last: Option<(Instant, u64)>,
    interval: Duration,
}

impl Throttle {
    fn new() -> Self { Self { last: None, interval: Duration::from_millis(250) } }

    fn allow(&mut self, now: Instant, done: u64, total: Option<u64>) -> bool {
        let complete = total.is_some_and(|t| done >= t);
        let ok = match self.last {
            None => true,
            Some((_, prev)) if done < prev => true,
            Some((t, _)) => complete || now.duration_since(t) >= self.interval,
        };
        if ok { self.last = Some((now, done)); }
        ok
    }
}

fn ensure_one(app: &AppHandle, cfg: &Config, id: &str) -> Result<PathBuf, String> {
    let spec = models::manifest().remove(id).ok_or(format!("modelo {id} fora do manifesto"))?;
    let dir = root(app)?.join("models").join(id);
    let app2 = app.clone();
    let file = id.to_string();
    // A função de progresso do core não informa o arquivo; o identificador do modelo o substitui.
    let mut throttle = Throttle::new();
    let base = std::env::var("GOW_MEET_MODELS_URL").unwrap_or_else(|_| cfg.models_base_url.clone());
    models::ensure(&spec, &base, &dir, &HttpFetch, &mut |done, total| {
        if throttle.allow(Instant::now(), done, total) {
            let _ = app2.emit("models-progress", serde_json::json!({"file": file, "done": done, "total": total}));
        }
    }).map_err(|e| {
        tracing::error!("baixar modelo {id}: {e:#}");
        format!("baixar modelo {id}: {e:#}")
    })?;
    Ok(dir)
}

/// Verifica/baixa os modelos uma vez por execução do app.
fn ensure_all(app: &AppHandle, st: &AppState) -> Result<ModelPaths, String> {
    let mut cached = lock(&st.models);
    if let Some(p) = cached.as_ref() { return Ok(p.clone()); }
    let whisper_dir = ensure_one(app, &st.config, &st.config.whisper_model)?;
    let spec = models::manifest().remove(&st.config.whisper_model)
        .ok_or(format!("modelo {} fora do manifesto", st.config.whisper_model))?;
    let opus_dir = ensure_one(app, &st.config, &st.config.opus_model)?;
    let paths = ModelPaths { whisper: whisper_dir.join(&spec.files[0].name), opus_dir };
    *cached = Some(paths.clone());
    Ok(paths)
}

#[tauri::command]
async fn ensure_models(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ensure_all(&app, &st).map(|_| ())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_session(app: AppHandle, title: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, title))
        .await.map_err(|e| e.to_string())?
}

fn start_blocking(app: &AppHandle, title: Option<String>) -> Result<String, String> {
    let st = app.state::<AppState>();
    {
        // Marca "starting" antes do carregamento lento: uma segunda chamada é rejeitada.
        let mut slot = lock(&st.slot);
        if !matches!(*slot, Slot::Idle) { return Err("já existe uma reunião em andamento".into()); }
        *slot = Slot::Starting;
    }
    match build_session(app, &st, title) {
        Ok((run, id)) => {
            *lock(&st.slot) = Slot::Running(Box::new(run));
            if let Some(w) = app.get_webview_window("overlay") { let _ = w.show(); }
            tracing::info!(session = %id, "sessão iniciada");
            Ok(id)
        }
        Err(e) => {
            *lock(&st.slot) = Slot::Idle;
            tracing::error!("falha ao iniciar a sessão: {e}");
            Err(e)
        }
    }
}

fn build_session(app: &AppHandle, st: &AppState, title: Option<String>) -> Result<(Running, String), String> {
    let cfg = &st.config;
    let paths = ensure_all(app, st)?;
    let asr = match WhisperTranscriber::new(&paths.whisper, &cfg.source_lang, true) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("GPU indisponível para o Whisper: {e:#}");
            let _ = app.emit("gow-meet", UiEvent::Error {
                message: "GPU indisponível: usando o processador, a tradução ficará mais lenta.".into() });
            WhisperTranscriber::new(&paths.whisper, &cfg.source_lang, false).map_err(|e| format!("{e:#}"))?
        }
    };
    let tr = Ct2Translator::new(&paths.opus_dir, cfg.opus_prefix.clone()).map_err(|e| format!("{e:#}"))?;
    let now = chrono::Local::now();
    let id = session_id(now);
    let started_ms = now.timestamp_millis() as u64;
    let meta = Meta {
        v: 1, id: id.clone(),
        title: title.filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("Reunião {}", now.format("%d/%m/%Y %H:%M"))),
        started: started_ms, ended: None,
        source_lang: cfg.source_lang.clone(), target_lang: cfg.target_lang.clone(),
        models: [("asr".to_string(), cfg.whisper_model.clone()), ("translate".to_string(), cfg.opus_model.clone())].into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    };
    let writer = SessionWriter::create(&root(app)?.join("sessions"), meta).map_err(|e| format!("{e:#}"))?;
    let pipeline = Arc::new(Pipeline::start(PipelineConfig::default(), Box::new(asr), Box::new(tr),
                                            writer, Arc::new(TauriSink(app.clone()))));
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut sys = SystemSource::new();
    if let Err(e) = sys.start(tx.clone()) {
        tracing::warn!("áudio do sistema indisponível: {e:#}");
        let _ = app.emit("gow-meet", UiEvent::Error { message: format!("{e:#}") });
    }
    let mut mic = MicSource::new();
    if let Err(e) = mic.start(tx) {
        sys.stop();
        if let Ok(p) = Arc::try_unwrap(pipeline) { let _ = p.finish(started_ms); }
        return Err(format!("{e:#}"));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let pump = {
        let (stop, pipeline, app) = (stop.clone(), pipeline.clone(), app.clone());
        std::thread::Builder::new().name("pump".into()).spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(CaptureEvent::Audio { source, samples }) => pipeline.feed(source, &samples),
                    Ok(CaptureEvent::Reconnecting(source)) => {
                        tracing::warn!(?source, "reconectando fonte de áudio");
                        let _ = app.emit("gow-meet", UiEvent::Reconnecting { source });
                    }
                    Ok(CaptureEvent::Error { source, message }) => {
                        tracing::error!(?source, "captura: {message}");
                        let _ = app.emit("gow-meet", UiEvent::Error { message });
                    }
                    Err(_) => {}
                }
            }
        }).map_err(|e| e.to_string())?
    };
    Ok((Running { pipeline, mic, sys, stop, pump, started: Instant::now(), started_ms }, id))
}

#[tauri::command]
async fn stop_session(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let mut run = {
            let mut slot = lock(&st.slot);
            match std::mem::replace(&mut *slot, Slot::Stopping) {
                Slot::Running(r) => r,
                other => { *slot = other; return Err("nenhuma reunião em andamento".to_string()); }
            }
        };
        // Ordem: parar a captura, encerrar o pump, finalizar o pipeline.
        run.mic.stop();
        run.sys.stop();
        run.stop.store(true, Ordering::SeqCst);
        let _ = run.pump.join();
        let ended = run.started_ms + run.started.elapsed().as_millis() as u64;
        let result = Arc::try_unwrap(run.pipeline)
            .map_err(|_| "pipeline ainda em uso".to_string())
            .and_then(|p| p.finish(ended).map_err(|e| format!("{e:#}")));
        *lock(&st.slot) = Slot::Idle;
        if let Some(w) = app.get_webview_window("overlay") { let _ = w.hide(); }
        match result {
            Ok(md) => {
                let transcript = md.display().to_string();
                let _ = app.emit("gow-meet", UiEvent::Stopped { transcript: transcript.clone() });
                tracing::info!("sessão encerrada");
                Ok(transcript)
            }
            Err(e) => {
                tracing::error!("falha ao encerrar a sessão: {e}");
                Err(e)
            }
        }
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

fn init_logs(root: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
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
        .setup(|app| {
            let root = root(app.handle())?;
            if let Some(guard) = init_logs(&root) { app.manage(guard); }
            let config = Config::load(&root.join("config.json"));
            app.manage(AppState { config, slot: Mutex::new(Slot::Idle), models: Mutex::new(None) });
            let now = chrono::Local::now().timestamp_millis() as u64;
            match recover_unfinished(&root.join("sessions"), now) {
                Ok(done) if !done.is_empty() => tracing::info!("{} sessão(ões) recuperada(s)", done.len()),
                Ok(_) => {}
                Err(e) => tracing::error!("recuperação: {e:#}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ensure_models, start_session, stop_session, list_sessions])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o gow-meet");
}

#[cfg(test)]
mod tests {
    use super::{Throttle, UiEvent};
    use gow_meet_core::session::{Line, Speaker, SpeakerSource};
    use std::time::{Duration, Instant};

    #[test]
    fn events_serialize_with_kind_tag() {
        let line = Line { id: "s0".into(), t0: 0, t1: 900, speaker: Speaker::system(),
            text: "Hello team.".into(), translation: Some("Olá, equipe.".into()), translation_error: false };
        let v = serde_json::to_value(UiEvent::Translated { line }).unwrap();
        assert_eq!(v["kind"], "translated");
        assert_eq!(v["line"]["speaker"]["label"], "Participante");
        let v = serde_json::to_value(UiEvent::Lag { ms: 6000 }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "lag", "ms": 6000}));
        let v = serde_json::to_value(UiEvent::Dropped { id: "s3".into() }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "dropped", "id": "s3"}));
        let v = serde_json::to_value(UiEvent::Reconnecting { source: SpeakerSource::Mic }).unwrap();
        assert_eq!(v["kind"], "reconnecting");
    }

    #[test]
    fn progress_is_throttled_but_first_and_final_pass() {
        let mut t = Throttle::new();
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(t.allow(t0, 10, Some(100)));
        assert!(!t.allow(ms(50), 20, Some(100)));
        assert!(!t.allow(ms(200), 30, Some(100)));
        assert!(t.allow(ms(260), 40, Some(100)));
        assert!(!t.allow(ms(270), 90, Some(100)));
        assert!(t.allow(ms(280), 100, Some(100)));
        // novo arquivo (done volta a baixar) emite de imediato
        assert!(t.allow(ms(285), 5, Some(50)));
    }
}
