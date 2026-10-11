mod config;
mod language;

use config::Config;
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::capture::{AudioSource, CaptureEvent, MicSource, SourceClock, SystemSource};
use gow_meet_core::lang::Lang;
use gow_meet_core::models::{self, HttpFetch};
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink, SpeakerId};
use gow_meet_core::session::{recover_unfinished, session_id, Line, Meta, SessionWriter, Speaker, SpeakerSource};
use gow_meet_core::speaker::TractEmbedder;
use gow_meet_core::tracker::SpeakerTracker;
use gow_meet_core::translate::Ct2Translator;
use gow_meet_core::voices::{VoiceLibrary, VoiceSummary, VOICES_FILE};
use language::{LangState, ProgressTo, Shown, Step};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

/// Contrato com o front-end: evento `"gow-meet"`, campo `kind` em camelCase.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiEvent {
    Partial { id: String, speaker: Speaker, t0: u64, text: String, translation: Option<String>, lang: Lang },
    Final { line: Line },
    Translated { line: Line },
    Dropped { id: String },
    Lag { ms: u64 },
    Error { message: String },
    Reconnecting { source: SpeakerSource },
    Stopped { transcript: String },
    /// Grupo de voz renomeado: as duas janelas trocam o rótulo de todas as falas dele.
    Speaker { voice: String, label: String },
    /// Idioma da reunião: `preparing` (com `progress` de 0 a 1 enquanto baixa), `active` ou `failed`.
    Language {
        lang: Lang,
        status: LangStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<f32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LangStatus { Preparing, Active, Failed }

fn emit_language(app: &AppHandle, lang: Lang, status: LangStatus, progress: Option<f32>) {
    let _ = app.emit("gow-meet", UiEvent::Language { lang, status, progress });
}

struct TauriSink(AppHandle);

impl Sink for TauriSink {
    fn emit(&self, ev: PipelineEvent) {
        let ui = match ev {
            PipelineEvent::Partial { id, speaker, t0, text, translation, lang, .. } =>
                UiEvent::Partial { id, speaker, t0, text, translation, lang },
            PipelineEvent::Final(line) => UiEvent::Final { line },
            PipelineEvent::Translated(line) => UiEvent::Translated { line },
            PipelineEvent::Dropped { id } => UiEvent::Dropped { id },
            PipelineEvent::Speaker { voice, label } => UiEvent::Speaker { voice, label },
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
    /// Identifica a sessão para o preparo do outro idioma, que roda fora do slot.
    id: String,
    lang: Arc<Mutex<LangState>>,
}

enum Slot {
    Idle,
    Starting,
    Running(Box<Running>),
    Stopping,
}

/// Modelos de um idioma já verificados (hash).
#[derive(Clone)]
struct PairPaths { whisper: PathBuf, opus_dir: PathBuf }

struct AppState {
    config: Config,
    slot: Mutex<Slot>,
    /// Pares já verificados nesta execução do app.
    pairs: Mutex<HashMap<Lang, PairPaths>>,
    /// Arquivo do modelo de voz já verificado; `None`: ainda não, ou falhou (a próxima
    /// reunião tenta de novo e, sem ele, a reunião segue sem identificar quem fala).
    speaker: Mutex<Option<PathBuf>>,
    /// Um download/verificação por vez: o preparo do outro idioma pode continuar depois
    /// que a reunião acaba e não pode escrever no mesmo `.part` que a reunião seguinte.
    ensuring: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sessões e config.json (no Windows, perfil Roaming).
fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().data_dir().map(|d| d.join("gow-meet")).map_err(|e| e.to_string())
}

/// Modelos (~700 MB) e logs: pasta local da máquina (no Windows, AppData\Local, fora do
/// perfil Roaming). No macOS é a mesma Application Support de `root`.
fn local_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().local_data_dir().map(|d| d.join("gow-meet")).map_err(|e| e.to_string())
}

/// Vozes guardadas: pasta local, como os modelos (fora do perfil Roaming no Windows).
fn voices_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(local_root(app)?.join(VOICES_FILE))
}

fn now_ms() -> u64 { chrono::Local::now().timestamp_millis() as u64 }

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

fn ensure_one(app: &AppHandle, cfg: &Config, id: &str, on_progress: &mut dyn FnMut(&str, u64, Option<u64>))
    -> Result<PathBuf, String> {
    let spec = models::manifest().remove(id).ok_or(format!("modelo {id} fora do manifesto"))?;
    let dir = local_root(app)?.join("models").join(id);
    let base = std::env::var("GOW_MEET_MODELS_URL").unwrap_or_else(|_| cfg.models_base_url.clone());
    models::ensure(&spec, &base, &dir, &HttpFetch::default(), on_progress).map_err(|e| {
        tracing::error!("baixar modelo {id}: {e:#}");
        format!("baixar modelo {id}: {e:#}")
    })?;
    Ok(dir)
}

/// Progresso para a janela principal (evento `models-progress`), ~4 por segundo por arquivo.
fn main_progress(app: &AppHandle) -> impl FnMut(&str, u64, Option<u64>) + '_ {
    let mut throttles: HashMap<String, Throttle> = HashMap::new();
    move |file, done, total| {
        let th = throttles.entry(file.to_string()).or_insert_with(Throttle::new);
        if th.allow(Instant::now(), done, total) {
            let _ = app.emit("models-progress", serde_json::json!({"file": file, "done": done, "total": total}));
        }
    }
}

/// Roda `f` com o lock de downloads.
fn serialized<T>(lock_: &Mutex<()>, f: impl FnOnce() -> T) -> T {
    let _g = lock(lock_);
    f()
}

/// Garante (baixa e confere o hash, uma vez por execução do app) os modelos de `lang`.
fn ensure_pair(app: &AppHandle, st: &AppState, lang: Lang, on_progress: &mut dyn FnMut(&str, u64, Option<u64>))
    -> Result<PairPaths, String> {
    cached_or_serialized(&st.pairs, &st.ensuring, lang, || {
        let pair = st.config.pair(lang);
        let whisper_dir = ensure_one(app, &st.config, &pair.whisper, on_progress)?;
        let file = models::manifest().remove(&pair.whisper).and_then(|s| s.files.into_iter().next())
            .ok_or(format!("modelo {} fora do manifesto", pair.whisper))?.name;
        let opus_dir = ensure_one(app, &st.config, &pair.opus, on_progress)?;
        Ok(PairPaths { whisper: whisper_dir.join(file), opus_dir })
    })
}

/// Valor de `key` no cache ou, se ausente, `make` com o lock de downloads (e guarda o
/// resultado). O cache é consultado antes do lock: um par já verificado não espera por
/// um download do outro idioma ainda em andamento (ex.: pedido numa reunião que já acabou).
fn cached_or_serialized<K: Eq + std::hash::Hash + Copy, V: Clone>(
    cache: &Mutex<HashMap<K, V>>, ensuring: &Mutex<()>, key: K, make: impl FnOnce() -> Result<V, String>,
) -> Result<V, String> {
    if let Some(v) = lock(cache).get(&key) { return Ok(v.clone()); }
    serialized(ensuring, || {
        if let Some(v) = lock(cache).get(&key) { return Ok(v.clone()); }
        let v = make()?;
        lock(cache).insert(key, v.clone());
        Ok(v)
    })
}

/// Os arquivos de `lang` já estão no disco (sem conferir o hash): ao iniciar, o outro
/// idioma só é pré-carregado se não precisar baixar nada.
fn pair_on_disk(app: &AppHandle, cfg: &Config, lang: Lang) -> bool {
    let Ok(root) = local_root(app) else { return false };
    let pair = cfg.pair(lang);
    let manifest = models::manifest();
    [&pair.whisper, &pair.opus].into_iter().all(|id| manifest.get(id.as_str()).is_some_and(|s|
        s.files.iter().all(|f| root.join("models").join(id).join(&f.name).is_file())))
}

/// Modelo de voz: verificado uma vez por execução; falha não impede a reunião.
fn ensure_speaker(app: &AppHandle, st: &AppState) -> Option<PathBuf> {
    if let Some(p) = lock(&st.speaker).clone() { return Some(p); }
    let cfg = &st.config;
    let Some(name) = models::manifest().get(&cfg.speaker_model).and_then(|s| s.files.first()).map(|f| f.name.clone()) else {
        tracing::warn!(model = %cfg.speaker_model, "modelo de voz fora do manifesto");
        return None;
    };
    let path = serialized(&st.ensuring, || ensure_one(app, cfg, &cfg.speaker_model, &mut main_progress(app)))
        .ok().map(|dir| dir.join(name));
    lock(&st.speaker).clone_from(&path);
    path
}

const GPU_UNAVAILABLE: &str = "GPU indisponível: usando o processador, a tradução ficará mais lenta.";

fn warn_gpu(app: &AppHandle) {
    let _ = app.emit("gow-meet", UiEvent::Error { message: GPU_UNAVAILABLE.into() });
}

/// Carrega Whisper (GPU; sem ela, processador) e Opus-MT de `lang`. O terceiro valor diz
/// se o Whisper caiu para o processador: quem chama avisa na tela, uma vez por reunião.
fn load_pair(cfg: &Config, lang: Lang, paths: &PairPaths)
    -> Result<(WhisperTranscriber, Ct2Translator, bool), String> {
    let (asr, cpu) = match WhisperTranscriber::new(&paths.whisper, lang.code(), true) {
        Ok(t) => (t, false),
        Err(e) => {
            tracing::warn!(lang = lang.code(), "GPU indisponível para o Whisper: {e:#}");
            (WhisperTranscriber::new(&paths.whisper, lang.code(), false).map_err(|e| format!("{e:#}"))?, true)
        }
    };
    let tr = Ct2Translator::new(&paths.opus_dir, cfg.pair(lang).prefix).map_err(|e| format!("{e:#}"))?;
    Ok((asr, tr, cpu))
}

pub const SPEAKER_UNAVAILABLE: &str =
    "Identificação de quem fala indisponível nesta reunião: os participantes aparecem como \"Participante\".";
pub const VOICES_CORRUPT: &str =
    "O arquivo de vozes guardadas estava corrompido: ele foi preservado e a lista de vozes recomeça vazia.";

/// Identificação de quem fala. Sem modelo, ou se ele não carrega, avisa uma vez e a
/// reunião segue sem ela.
fn speaker_setup(app: &AppHandle, cfg: &Config, model: Option<&Path>) -> Option<SpeakerId> {
    let warn = |message: &str| { let _ = app.emit("gow-meet", UiEvent::Error { message: message.into() }); };
    let embedder = match model.map(TractEmbedder::new) {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            tracing::warn!("modelo de voz não carregou: {e:#}");
            warn(SPEAKER_UNAVAILABLE);
            return None;
        }
        None => {
            warn(SPEAKER_UNAVAILABLE);
            return None;
        }
    };
    let path = match voices_path(app) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("pasta das vozes guardadas indisponível: {e}");
            warn(SPEAKER_UNAVAILABLE);
            return None;
        }
    };
    let (library, corrupt) = VoiceLibrary::load(&path, now_ms());
    if corrupt.is_some() { warn(VOICES_CORRUPT); }
    tracing::info!(vozes = library.len(), "identificação de vozes ligada");
    Some(SpeakerId { embedder: Box::new(embedder),
                     tracker: SpeakerTracker::new(cfg.tracker(), &cfg.speaker_model, library) })
}

#[tauri::command]
async fn ensure_models(app: AppHandle, lang: Lang) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ensure_pair(&app, &st, lang, &mut main_progress(&app))?;
        ensure_speaker(&app, &st);
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_session(app: AppHandle, title: Option<String>, lang: Lang) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, title, lang))
        .await.map_err(|e| e.to_string())?
}

fn start_blocking(app: &AppHandle, title: Option<String>, lang: Lang) -> Result<String, String> {
    let st = app.state::<AppState>();
    {
        // Marca "starting" antes do carregamento lento: uma segunda chamada é rejeitada.
        let mut slot = lock(&st.slot);
        if !matches!(*slot, Slot::Idle) { return Err("já existe uma reunião em andamento".into()); }
        *slot = Slot::Starting;
    }
    match build_session(app, &st, title, lang) {
        Ok((run, id)) => {
            *lock(&st.slot) = Slot::Running(Box::new(run));
            emit_language(app, lang, LangStatus::Active, None);
            preload_other(app, &st, lang);
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

fn build_session(app: &AppHandle, st: &AppState, title: Option<String>, lang: Lang)
    -> Result<(Running, String), String> {
    let cfg = &st.config;
    let paths = ensure_pair(app, st, lang, &mut main_progress(app))?;
    let speaker_model = ensure_speaker(app, st);
    let (asr, tr, cpu) = load_pair(cfg, lang, &paths)?;
    if cpu { warn_gpu(app); }
    let speakers = speaker_setup(app, cfg, speaker_model.as_deref());
    let pair = cfg.pair(lang);
    let mut model_ids = vec![("asr".to_string(), pair.whisper.clone()), ("translate".to_string(), pair.opus.clone())];
    if speakers.is_some() { model_ids.push(("speaker".to_string(), cfg.speaker_model.clone())); }
    let now = chrono::Local::now();
    let id = session_id(now);
    let started_ms = now.timestamp_millis() as u64;
    let meta = Meta {
        v: 1, id: id.clone(),
        title: title.filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("Reunião {}", now.format("%d/%m/%Y %H:%M"))),
        started: started_ms, ended: None,
        source_lang: lang.code().into(), target_lang: lang.other().code().into(),
        models: model_ids.into_iter().collect(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    };
    let writer = SessionWriter::create(&root(app)?.join("sessions"), meta).map_err(|e| format!("{e:#}"))?;
    let pipeline = Arc::new(Pipeline::start_with_speakers(PipelineConfig { lang, ..PipelineConfig::default() }, Box::new(asr), Box::new(tr),
                                                          speakers, writer, Arc::new(TauriSink(app.clone()))));
    let (tx, rx) = crossbeam_channel::unbounded();
    // Início da linha do tempo das fontes: antes de abri-las.
    let t0 = Instant::now();
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
    let mut lang_state = LangState::new(lang);
    // O aviso de GPU do par inicial já apareceu: o segundo par não repete.
    if cpu { lang_state.warn_gpu_once(); }
    let stop = Arc::new(AtomicBool::new(false));
    let pump = {
        let (stop, pipeline, app) = (stop.clone(), pipeline.clone(), app.clone());
        std::thread::Builder::new().name("pump".into()).spawn(move || {
            // Lacunas de entrega (reconexão, ScreenCaptureKit calado) viram silêncio, para o
            // t0 de cada fonte acompanhar o relógio e a ordem das falas na janela flutuante.
            let mut clock = SourceClock::default();
            // O ScreenCaptureKit não entrega nada no silêncio: a cada ~100 ms completa as
            // fontes atrasadas, para o segmentador fechar a última fala do participante.
            let mut last_fill = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(CaptureEvent::Audio { source, samples }) =>
                        clock.align(source, t0.elapsed(), &samples, |x| pipeline.feed(source, x)),
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
                if last_fill.elapsed() >= Duration::from_millis(100) {
                    last_fill = Instant::now();
                    clock.fill_to(t0.elapsed(), |source, x| pipeline.feed(source, x));
                }
            }
        }).map_err(|e| e.to_string())?
    };
    Ok((Running { pipeline, mic, sys, stop, pump, started: Instant::now(), started_ms,
                  id: id.clone(), lang: Arc::new(Mutex::new(lang_state)) }, id))
}

/// Pré-carrega o outro idioma em segundo plano se ele já está no disco (troca instantânea).
fn preload_other(app: &AppHandle, st: &AppState, lang: Lang) {
    let other = lang.other();
    if !pair_on_disk(app, &st.config, other) { return; }
    let slot = lock(&st.slot);
    let Slot::Running(run) = &*slot else { return };
    if lock(&run.lang).preload(other) { spawn_prepare(app.clone(), run.id.clone(), run.lang.clone(), other); }
}

/// A sessão `id` ainda está rodando: o preparo do outro idioma só entrega modelos a ela.
fn same_session<'a>(slot: &'a Slot, id: &str) -> Option<&'a Running> {
    match slot { Slot::Running(run) if run.id == id => Some(run), _ => None }
}

fn switch_now(app: &AppHandle, run: &Running, state: &mut LangState, lang: Lang) {
    run.pipeline.set_language(lang);
    state.switched(lang);
    emit_language(app, lang, LangStatus::Active, None);
}

/// Prepara o par de `lang` numa thread própria: baixa (se preciso), confere, carrega e
/// entrega ao pipeline da sessão `session`. A thread nunca guarda o pipeline: se a sessão
/// acabou (ou é outra), os modelos são descartados.
fn spawn_prepare(app: AppHandle, session: String, state: Arc<Mutex<LangState>>, lang: Lang) {
    let spawned = std::thread::Builder::new().name("prepare-lang".into()).spawn(move || {
        let t0 = Instant::now();
        let st = app.state::<AppState>();
        let mut throttle = Throttle::new();
        let mut main = main_progress(&app);
        let mut on_progress = |file: &str, done: u64, total: Option<u64>| {
            // Emite com o lock do idioma: um cancelamento não passa entre a checagem e o
            // evento (senão um "preparing" atrasado ficaria na janela flutuante).
            let ls = lock(&state);
            match ls.progress_to(lang) {
                ProgressTo::Language => {
                    let Some(total) = total.filter(|t| *t > 0) else { return };
                    if throttle.allow(Instant::now(), done, Some(total)) {
                        emit_language(&app, lang, LangStatus::Preparing, Some(done as f32 / total as f32));
                    }
                }
                // A reunião acabou: uma reunião nova pode estar esperando por este download.
                ProgressTo::Main => main(file, done, total),
                ProgressTo::Nowhere => {}
            }
        };
        let loaded = ensure_pair(&app, &st, lang, &mut on_progress)
            .and_then(|paths| load_pair(&st.config, lang, &paths));
        let slot = lock(&st.slot);
        let Some(run) = same_session(&slot, &session) else { return };
        let mut ls = lock(&run.lang);
        match loaded {
            Ok((asr, tr, cpu)) => {
                if cpu && ls.warn_gpu_once() { warn_gpu(&app); }
                tracing::info!(lang = lang.code(), ms = t0.elapsed().as_millis() as u64, "par de idioma pronto");
                let pair = st.config.pair(lang);
                run.pipeline.add_models(BTreeMap::from([
                    (format!("asr_{}", lang.code()), pair.whisper), (format!("translate_{}", lang.code()), pair.opus)]));
                run.pipeline.add_language(lang, Box::new(asr), Box::new(tr));
                if let Some(l) = ls.loaded(lang) { switch_now(&app, run, &mut ls, l); }
            }
            Err(e) => {
                tracing::warn!(lang = lang.code(), "par de idioma não preparado: {e}");
                if ls.failed(lang) {
                    emit_language(&app, lang, LangStatus::Failed, None);
                    emit_language(&app, ls.active(), LangStatus::Active, None);
                }
            }
        }
    });
    if let Err(e) = spawned { tracing::error!("thread de preparo do idioma não iniciou: {e}"); }
}

/// Troca o idioma falado durante a reunião (janela principal e flutuante).
#[tauri::command]
async fn set_language(app: AppHandle, lang: Lang) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let slot = lock(&st.slot);
        let Slot::Running(run) = &*slot else { return Err("nenhuma reunião em andamento".to_string()) };
        let mut ls = lock(&run.lang);
        let step = ls.request(lang);
        tracing::info!(lang = lang.code(), ?step, "pedido de troca de idioma");
        match step {
            // Reenvia o estado atual: um clique sempre ressincroniza as janelas.
            Step::Nothing => match ls.shown() {
                Shown::Active(l) => emit_language(&app, l, LangStatus::Active, None),
                Shown::Preparing(l) => emit_language(&app, l, LangStatus::Preparing, None),
            },
            Step::Stay(l) => emit_language(&app, l, LangStatus::Active, None),
            Step::Switch(l) => switch_now(&app, run, &mut ls, l),
            Step::Wait(l) => emit_language(&app, l, LangStatus::Preparing, None),
            Step::Prepare(l) => {
                emit_language(&app, l, LangStatus::Preparing, None);
                spawn_prepare(app.clone(), run.id.clone(), run.lang.clone(), l);
            }
        }
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn stop_session(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || stop_blocking(&app))
        .await.map_err(|e| e.to_string())?
}

fn stop_blocking(app: &AppHandle) -> Result<String, String> {
    let result = end_session(&app.state::<AppState>());
    if let Some(w) = app.get_webview_window("overlay") { let _ = w.hide(); }
    match result {
        Ok(md) => {
            let _ = app.emit("gow-meet", UiEvent::Stopped { transcript: md.clone() });
            tracing::info!("sessão encerrada");
            Ok(md)
        }
        Err(e) => {
            if !matches!(e, EndError::NotRunning) { tracing::error!("falha ao encerrar a sessão: {e}"); }
            Err(e.to_string())
        }
    }
}

#[derive(Debug)]
enum EndError { NotRunning, Failed(String) }

impl std::fmt::Display for EndError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndError::NotRunning => f.write_str("nenhuma reunião em andamento"),
            EndError::Failed(e) => f.write_str(e),
        }
    }
}

/// Encerra a sessão em andamento sem tocar em janelas nem emitir eventos: serve também
/// ao `RunEvent::Exit`, quando o laço de eventos já está sendo desmontado. Idempotente.
fn end_session(st: &AppState) -> Result<String, EndError> {
    let mut run = {
        let mut slot = lock(&st.slot);
        match std::mem::replace(&mut *slot, Slot::Stopping) {
            Slot::Running(r) => r,
            other => { *slot = other; return Err(EndError::NotRunning); }
        }
    };
    // Fora do slot (ordem slot → idioma): um preparo do outro idioma ainda em andamento
    // para de mostrar progresso, que senão apareceria na reunião seguinte.
    lock(&run.lang).close();
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
    result.map(|md| md.display().to_string()).map_err(EndError::Failed)
}

/// Cmd+Q / Dock → Encerrar: o macOS chama `applicationWillTerminate`, que vira
/// `RunEvent::Exit` de forma síncrona, antes de o processo sair. Fecha a sessão aqui.
fn shutdown_on_exit(app: &AppHandle) {
    let st = app.state::<AppState>();
    // Um encerramento em outra thread (botão ou fechar janela) termina antes; limite de 30 s.
    let deadline = Instant::now() + Duration::from_secs(30);
    while matches!(*lock(&st.slot), Slot::Stopping) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    // Uma partida ainda em andamento (`Starting`) não é esperada: a recuperação na
    // próxima abertura fecha a sessão incompleta.
    match end_session(&st) {
        Ok(_) => tracing::info!("sessão encerrada ao sair do app"),
        Err(EndError::NotRunning) => {}
        Err(e) => tracing::error!("falha ao encerrar a sessão ao sair: {e}"),
    }
}

/// Fechar a janela principal encerra a sessão (se houver) e o app: nunca deixa a captura sem UI.
fn shutdown_on_close(app: AppHandle) {
    std::thread::spawn(move || {
        let st = app.state::<AppState>();
        // Uma partida em andamento termina antes de ser parada.
        while matches!(*lock(&st.slot), Slot::Starting | Slot::Stopping) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Err(e) = stop_blocking(&app) {
            tracing::debug!("encerramento sem sessão ativa: {e}");
        }
        if let Some(w) = app.get_webview_window("overlay") { let _ = w.hide(); }
        app.exit(0);
    });
}

/// Item da lista de reuniões: o `meta.json` mais o caminho do `transcript.md`, quando
/// ele já existe (sessão encerrada), para a janela principal abri-lo.
#[derive(Debug, Serialize)]
struct SessionItem {
    #[serde(flatten)]
    meta: Meta,
    transcript: Option<String>,
}

fn read_sessions(dir: &Path) -> Vec<SessionItem> {
    let mut out: Vec<SessionItem> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter_map(|e| {
            let meta: Meta = serde_json::from_slice(&std::fs::read(e.path().join("meta.json")).ok()?).ok()?;
            let md = e.path().join("transcript.md");
            let transcript = md.is_file().then(|| md.to_string_lossy().into_owned());
            Some(SessionItem { meta, transcript })
        })
        .collect();
    out.sort_by(|a, b| b.meta.started.cmp(&a.meta.started));
    out
}

#[tauri::command]
fn list_sessions(app: AppHandle) -> Result<Vec<SessionItem>, String> {
    Ok(read_sessions(&root(&app)?.join("sessions")))
}

/// Dá nome a um grupo de voz durante a reunião (janela flutuante). O slot fica travado
/// durante a renomeação: o encerramento espera e o pipeline não muda de dono no meio.
#[tauri::command]
async fn rename_voice(app: AppHandle, voice: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let slot = lock(&st.slot);
        let Slot::Running(run) = &*slot else { return Err("nenhuma reunião em andamento".to_string()) };
        run.pipeline.rename_voice(&voice, &name).map_err(|e| format!("{e:#}"))
    }).await.map_err(|e| e.to_string())?
}

/// Vozes guardadas, sem impressão de voz. Só leitura: um arquivo corrompido vira erro
/// aqui e é preservado na próxima reunião ou alteração.
#[tauri::command]
fn list_voices(app: AppHandle) -> Result<Vec<VoiceSummary>, String> {
    VoiceLibrary::read_summaries(&voices_path(&app)?).map_err(|e| format!("{e:#}"))
}

/// Alterações nas vozes guardadas só fora de reunião: durante a reunião o rastreador tem a
/// biblioteca em memória e a regravaria ao encerrar (uma voz apagada voltaria).
fn voices_locked(slot: &Slot) -> Option<&'static str> {
    (!matches!(slot, Slot::Idle)).then_some("Encerre a reunião para alterar as vozes guardadas.")
}

/// Carrega, altera e grava (só se houve mudança). Chamada com o slot travado.
fn edit_library(path: &Path, f: impl FnOnce(&mut VoiceLibrary) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let (mut library, _) = VoiceLibrary::load(path, now_ms());
    f(&mut library)?;
    library.save()
}

/// Apaga tudo (voices.json, .tmp e cópias preservadas) e NÃO regrava: o estado final é
/// sem voices.json.
fn wipe_library(path: &Path) -> anyhow::Result<()> {
    let (mut library, _) = VoiceLibrary::load(path, now_ms());
    library.delete_all()
}

/// O slot fica travado durante a alteração: nenhuma reunião começa no meio dela.
fn with_voices_locked(app: &AppHandle, f: impl FnOnce(&Path) -> anyhow::Result<()>) -> Result<(), String> {
    let st = app.state::<AppState>();
    let slot = lock(&st.slot);
    if let Some(msg) = voices_locked(&slot) { return Err(msg.into()); }
    f(&voices_path(app)?).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn rename_saved_voice(app: AppHandle, id: String, name: String) -> Result<(), String> {
    with_voices_locked(&app, |p| edit_library(p, |l| l.rename(&id, &name)))
}

#[tauri::command]
fn delete_voice(app: AppHandle, id: String) -> Result<(), String> {
    with_voices_locked(&app, |p| edit_library(p, |l| l.delete(&id)))?;
    tracing::info!(id = %id, "voz guardada apagada");
    Ok(())
}

#[tauri::command]
fn delete_all_voices(app: AppHandle) -> Result<(), String> {
    with_voices_locked(&app, wipe_library)?;
    tracing::info!("todas as vozes guardadas apagadas");
    Ok(())
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
            let local = local_root(app.handle())?;
            if let Some(guard) = init_logs(&local) { app.manage(guard); }
            tracing::info!(dados = %root.display(), locais = %local.display(), "pastas do app");
            let config = Config::load(&root.join("config.json"));
            app.manage(AppState { config, slot: Mutex::new(Slot::Idle), pairs: Mutex::new(HashMap::new()),
                                 speaker: Mutex::new(None), ensuring: Mutex::new(()) });
            match recover_unfinished(&root.join("sessions")) {
                Ok(done) if !done.is_empty() => tracing::info!("{} sessão(ões) recuperada(s)", done.len()),
                Ok(_) => {}
                Err(e) => tracing::error!("recuperação: {e:#}"),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    "main" => {
                        api.prevent_close();
                        shutdown_on_close(window.app_handle().clone());
                    }
                    // Cmd+W na flutuante só a esconde: ela volta na próxima sessão.
                    "overlay" => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ensure_models, start_session, stop_session, list_sessions,
            rename_voice, list_voices, rename_saved_voice, delete_voice, delete_all_voices, set_language
        ])
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o GOW Meet")
        .run(|app, event| {
            if let RunEvent::Exit = event { shutdown_on_exit(app); }
        });
}

#[cfg(test)]
mod tests {
    use super::{cached_or_serialized, lock, read_sessions, same_session, serialized, voices_locked, wipe_library, LangStatus, Slot, Throttle, UiEvent};
    use gow_meet_core::lang::Lang;
    use gow_meet_core::session::{Line, Meta, Speaker, SpeakerSource};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[test]
    fn events_serialize_with_kind_tag() {
        let line = Line { id: "s0".into(), t0: 0, t1: 900, speaker: Speaker::system(), lang: Lang::En,
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
        let v = serde_json::to_value(UiEvent::Speaker { voice: "v2".into(), label: "Ana".into() }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "speaker", "voice": "v2", "label": "Ana"}));
        let line = Line { id: "s1".into(), t0: 0, t1: 900, speaker: Speaker::tracked("v2", "Participante 2"), lang: Lang::En,
            text: "Hi.".into(), translation: None, translation_error: false };
        let v = serde_json::to_value(UiEvent::Final { line }).unwrap();
        assert_eq!(v["line"]["speaker"], serde_json::json!({"source": "system", "label": "Participante 2", "voice": "v2"}));
        let mic = Line { id: "m0".into(), t0: 0, t1: 1, speaker: Speaker::mic(), lang: Lang::En, text: "Oi.".into(),
            translation: None, translation_error: false };
        let v = serde_json::to_value(UiEvent::Final { line: mic }).unwrap();
        assert!(v["line"]["speaker"].get("voice").is_none());
        let v = serde_json::to_value(UiEvent::Language { lang: Lang::Pt, status: LangStatus::Preparing, progress: Some(0.5) }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "language", "lang": "pt", "status": "preparing", "progress": 0.5}));
        let v = serde_json::to_value(UiEvent::Language { lang: Lang::En, status: LangStatus::Active, progress: None }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "language", "lang": "en", "status": "active"}));
        let v = serde_json::to_value(UiEvent::Partial { id: "s0".into(), speaker: Speaker::system(), t0: 0,
            text: "Olá".into(), translation: None, lang: Lang::Pt }).unwrap();
        assert_eq!(v["lang"], "pt");
    }

    #[test]
    fn ensure_is_serialized() {
        let st = std::sync::Arc::new(std::sync::Mutex::new(()));
        let inside = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hs: Vec<_> = (0..4).map(|_| {
            let (st, inside) = (st.clone(), inside.clone());
            std::thread::spawn(move || serialized(&st, || {
                assert_eq!(inside.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                std::thread::sleep(Duration::from_millis(20));
                inside.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }))
        }).collect();
        for h in hs { h.join().unwrap(); }
    }

    #[test]
    fn cached_pair_does_not_wait_for_another_download() {
        let cache = std::sync::Arc::new(Mutex::new(HashMap::from([(Lang::En, 1u32)])));
        let ensuring = std::sync::Arc::new(Mutex::new(()));
        let busy = lock(&ensuring); // download do português em andamento
        let (tx, rx) = std::sync::mpsc::channel();
        let h = {
            let (cache, ensuring) = (cache.clone(), ensuring.clone());
            std::thread::spawn(move || {
                let _ = tx.send(cached_or_serialized(&cache, &ensuring, Lang::En, || Err("baixou de novo".into())));
            })
        };
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).expect("esperou o lock"), Ok(1));
        drop(busy);
        h.join().unwrap();
        // Ausente: roda com o lock e guarda.
        assert_eq!(cached_or_serialized(&cache, &ensuring, Lang::Pt, || Ok(2)), Ok(2));
        assert_eq!(cached_or_serialized(&cache, &ensuring, Lang::Pt, || Err("de novo".into())), Ok(2));
    }

    #[test]
    fn prepare_for_ended_session_is_ignored() {
        assert!(same_session(&Slot::Idle, "a").is_none());
        assert!(same_session(&Slot::Stopping, "a").is_none());
    }

    /// O `PathResolver` do Tauri usa `dirs::data_dir` (root) e `dirs::data_local_dir`
    /// (local_root). No macOS os dois são Application Support: os modelos já baixados
    /// continuam em `.../gow-meet/models`.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_models_and_logs_stay_in_application_support() {
        let data = dirs::data_dir().unwrap();
        let local = dirs::data_local_dir().unwrap();
        eprintln!("data_dir = {}\ndata_local_dir = {}", data.display(), local.display());
        assert_eq!(data, local);
        assert!(local.ends_with("Library/Application Support"));
    }

    #[test]
    fn saved_voices_only_change_outside_a_meeting() {
        assert_eq!(voices_locked(&Slot::Idle), None);
        assert!(voices_locked(&Slot::Starting).is_some());
        assert!(voices_locked(&Slot::Stopping).is_some());
    }

    #[test]
    fn delete_all_leaves_no_voices_file_and_no_backups() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("voices.json");
        std::fs::write(&p, "{ corrompido").unwrap();
        std::fs::write(d.path().join("voices.json.corrompido-1"), "x").unwrap();
        wipe_library(&p).unwrap();
        let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(left.is_empty(), "sobrou {left:?}");
        // Sem arquivo nenhum também funciona e não cria nada.
        wipe_library(&p).unwrap();
        assert!(!p.exists());
    }

    fn meta(id: &str, started: u64, ended: Option<u64>) -> Meta {
        Meta { v: 1, id: id.into(), title: format!("T {id}"), started, ended,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() }
    }

    #[test]
    fn list_sessions_includes_transcript_path_when_present() {
        let root = tempfile::tempdir().unwrap();
        for (m, md) in [(meta("a", 1, Some(2)), true), (meta("b", 5, None), false)] {
            let dir = root.path().join(&m.id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("meta.json"), serde_json::to_vec(&m).unwrap()).unwrap();
            if md { std::fs::write(dir.join("transcript.md"), "# T").unwrap(); }
        }
        std::fs::create_dir_all(root.path().join("lixo")).unwrap();
        let items = read_sessions(root.path());
        assert_eq!(items.len(), 2);
        let v = serde_json::to_value(&items).unwrap();
        assert_eq!(v[0]["id"], "b");
        assert_eq!(v[0]["ended"], serde_json::Value::Null);
        assert_eq!(v[0]["transcript"], serde_json::Value::Null);
        assert_eq!(v[1]["id"], "a");
        assert_eq!(v[1]["title"], "T a");
        let md = root.path().join("a").join("transcript.md");
        assert_eq!(v[1]["transcript"], md.to_string_lossy().as_ref());
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
