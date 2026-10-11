import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState, type ReactNode } from "react";
import logo from "./assets/brand/gow-horizontal-negativo.png";
import { IconDocument, IconHeadphones, IconMegaphone } from "./icons";
import { LANG_NAME, LANGS, loadLang, other, preparingText, saveLang, type Lang, type Language } from "./lib/lang";
import { formatDate, formatDuration, percent } from "./lib/format";
import type { PermissionKind } from "./lib/transcript";
import { voicesMessage } from "./lib/status";
import { resetTranscript, useGowMeet } from "./lib/useGowMeet";
import { SavedVoices, type SavedVoice, type VoicesUi } from "./SavedVoices";

/** Item de `list_sessions`: `meta.json` + caminho do `transcript.md` (quando já existe). */
export type Session = {
  id: string; title: string; started: number; ended: number | null; transcript: string | null;
};
export type Progress = { file: string; done: number; total: number | null };
export type Phase = "idle" | "starting" | "running" | "stopping";

const SETTINGS: Record<PermissionKind, { url: string; label: string }> = {
  screen: {
    url: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
    label: "Abrir Gravação de Tela e Áudio do Sistema",
  },
  mic: {
    url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
    label: "Abrir Microfone",
  },
};

const STATUS: Record<Phase, string> = {
  idle: "Pronto", starting: "Preparando", running: "Em reunião", stopping: "Encerrando",
};

const mb = (n: number) => Math.round(n / 1e6);

/** Relê as vozes guardadas; um erro (ex.: arquivo corrompido) aparece na seção. */
const loadVoices = (set: (v: SavedVoice[]) => void, setError: (e: string | null) => void) =>
  invoke<SavedVoice[]>("list_voices")
    .then((v) => { set(v); setError(null); })
    .catch((e) => setError(String(e)));

export function MainWindow() {
  const state = useGowMeet();
  const [title, setTitle] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [sessions, setSessions] = useState<Session[]>([]);

  const [voices, setVoices] = useState<SavedVoice[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = () => {
    invoke<Session[]>("list_sessions").then(setSessions).catch(() => {});
    loadVoices(setVoices, setLoadError);
  };
  /** Ação nas vozes guardadas: o erro fica na seção até a próxima ação que der certo;
   *  reler a lista não o apaga. */
  const voiceAction = (cmd: string, args?: Record<string, unknown>) =>
    invoke(cmd, args)
      .then(() => setActionError(null))
      .catch((e) => setActionError(String(e)))
      .finally(() => loadVoices(setVoices, setLoadError));
  useEffect(() => {
    refresh();
    const un = listen<Progress>("models-progress", (e) =>
      setProgress((p) => ({ ...p, [e.payload.file]: e.payload })));
    return () => { un.then((f) => f()); };
  }, []);

  const [chosen, setChosen] = useState<Lang>(loadLang);
  // A troca feita na flutuante também vale como última escolha.
  const active = state.language.active;
  useEffect(() => {
    if (active) { setChosen(active); saveLang(active); }
  }, [active]);
  const onLang = (l: Lang) => {
    if (phase === "running") {
      invoke("set_language", { lang: l }).catch((e) => setError(String(e)));
      return;
    }
    setChosen(l);
    saveLang(l);
  };

  async function start() {
    setPhase("starting"); setError(null); setProgress({});
    try {
      await invoke("ensure_models", { lang: chosen });
      await resetTranscript(); // nova sessão: ids s0/m0 voltam a existir
      await invoke("start_session", { title: title || null, lang: chosen });
      setPhase("running");
    } catch (e) {
      setError(String(e));
      setPhase("idle");
    } finally {
      setProgress({});
      refresh();
    }
  }

  async function stop() {
    setPhase("stopping"); setError(null);
    try {
      await invoke("stop_session");
      setPhase("idle");
    } catch (e) {
      setError(String(e));
      setPhase("idle");
    } finally {
      refresh();
    }
  }

  const open = (path: string) => openPath(path).catch((e) => setError(String(e)));

  return (
    <MainView
      phase={phase} title={title} onTitle={setTitle} onStart={start} onStop={stop}
      progress={Object.values(progress)} error={error} notice={state.notice}
      permissions={state.permissions}
      onOpenSettings={(k) => openUrl(SETTINGS[k].url).catch((e) => setError(String(e)))}
      lastTranscript={state.transcript} sessions={sessions} onOpen={open}
      voices={voices} voicesError={voicesMessage(actionError, loadError)}
      onRenameVoice={(id, name) => voiceAction("rename_saved_voice", { id, name })}
      onDeleteVoice={(id) => voiceAction("delete_voice", { id })}
      onDeleteAllVoices={() => voiceAction("delete_all_voices")}
      lang={phase === "running" ? active ?? chosen : chosen} language={state.language} onLang={onLang}
    />
  );
}

export type MainViewProps = {
  phase: Phase;
  title: string;
  onTitle: (t: string) => void;
  onStart: () => void;
  onStop: () => void;
  progress: Progress[];
  error: string | null;
  notice: string | null;
  permissions: Partial<Record<PermissionKind, string>>;
  onOpenSettings: (k: PermissionKind) => void;
  lastTranscript: string | null;
  sessions: Session[];
  onOpen: (path: string) => void;
  voices: SavedVoice[];
  voicesError: string | null;
  onRenameVoice: (id: string, name: string) => void;
  onDeleteVoice: (id: string) => void;
  onDeleteAllVoices: () => void;
  /** Idioma mostrado no seletor: o escolhido (parado) ou o que vale na reunião. */
  lang: Lang;
  language: Language;
  onLang: (l: Lang) => void;
  /** Só para o modo demo: estado inicial da seção de vozes. */
  voicesUi?: VoicesUi;
};

/** Apresentação da janela principal, sem acesso ao backend (usada também no modo demo). */
export function MainView(p: MainViewProps) {
  const { phase } = p;
  const permissionKinds = Object.keys(p.permissions) as PermissionKind[];
  return (
    <div className="app">
      <div className="band" aria-hidden />
      <div className="shell">
        <header className="topbar">
          <div className="brand">
            <img className="brand-logo" src={logo} alt="GOW Aprimorando Negócios" draggable={false} />
            <span className="product">MEET</span>
          </div>
          <span className={`pill pill-${phase}`}>
            {phase === "running" && <span className="dot" />}
            {STATUS[phase]}
          </span>
        </header>

        <main className="content">
          <section className="section">
            <h2 className="title">Nova reunião</h2>
            <div className="controls">
              <input className="field" placeholder="Título da reunião (opcional)" value={p.title}
                disabled={phase !== "idle"} onChange={(e) => p.onTitle(e.target.value)} />
              <button className="btn btn-primary" disabled={phase !== "idle"} onClick={p.onStart}>
                {phase === "starting" ? "Preparando…" : "Iniciar"}
              </button>
              <button className="btn btn-outline" disabled={phase !== "running"} onClick={p.onStop}>
                {phase === "stopping" ? "Encerrando…" : "Encerrar"}
              </button>
            </div>
            <LangPicker phase={phase} lang={p.lang} language={p.language} onLang={p.onLang} />
            <p className="note">
              <IconMegaphone />
              <span>Antes de começar, avise os participantes de que a reunião será transcrita.</span>
            </p>
            <p className="note">
              <IconHeadphones />
              <span>
                Use fones de ouvido: no alto-falante, a fala do participante também entra pelo
                microfone e aparece duplicada como &apos;Você&apos;.
              </span>
            </p>

            {phase === "starting" && p.progress.length > 0 && (
              <ul className="downloads">
                {p.progress.map((f) => {
                  const pct = percent(f.done, f.total);
                  return (
                    <li key={f.file} className="download">
                      <div className="download-head">
                        <span className="download-file">{f.file}</span>
                        <span className="download-amount">
                          {mb(f.done)} MB{f.total ? ` de ${mb(f.total)} MB` : ""}
                          {pct !== null && <strong>{pct}%</strong>}
                        </span>
                      </div>
                      <div className="bar"><div className="bar-fill" style={{ width: `${pct ?? 0}%` }} /></div>
                    </li>
                  );
                })}
              </ul>
            )}

            {p.error && <Card label="Não foi possível concluir">{p.error}</Card>}
            {p.notice && <Card label="Aviso">{p.notice}</Card>}
            {permissionKinds.map((k) => (
              <Card key={k} label="Permissão necessária">
                <p className="card-message">{p.permissions[k]}</p>
                <p className="card-guide">
                  Habilite o GOW Meet em Ajustes do Sistema → Privacidade e Segurança e inicie de novo.
                </p>
                <button className="btn btn-outline btn-small" onClick={() => p.onOpenSettings(k)}>
                  {SETTINGS[k].label}
                </button>
              </Card>
            ))}
          </section>

          <section className="section">
            <div className="section-head">
              <h2 className="title">Reuniões</h2>
              {p.lastTranscript && phase === "idle" && (
                <button className="btn btn-ghost btn-small" onClick={() => p.onOpen(p.lastTranscript!)}>
                  <IconDocument /> Abrir última transcrição
                </button>
              )}
            </div>
            {p.sessions.length === 0
              ? <p className="empty">Nenhuma reunião salva ainda.</p>
              : (
                <ul className="sessions">
                  {p.sessions.map((s) => (
                    <li key={s.id}>
                      <button className="session" disabled={!s.transcript}
                        title={s.transcript ? "Abrir a transcrição" : undefined}
                        onClick={() => s.transcript && p.onOpen(s.transcript)}>
                        <span className="session-title">{s.title}</span>
                        <span className="session-date">{formatDate(s.started)}</span>
                        <span className="session-duration">
                          {s.ended ? formatDuration(s.started, s.ended) : "—"}
                        </span>
                        <span className={`session-status ${s.ended ? "" : "live"}`}>
                          {s.ended ? "encerrada" : "em andamento"}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
          </section>

          <SavedVoices voices={p.voices} error={p.voicesError} locked={phase !== "idle"}
            onRename={p.onRenameVoice} onDelete={p.onDeleteVoice} onDeleteAll={p.onDeleteAllVoices}
            initial={p.voicesUi} />
        </main>
      </div>
    </div>
  );
}

/** "Idioma falado": antes de iniciar, escolhe o idioma da reunião; durante, troca. */
function LangPicker({ phase, lang, language, onLang }: {
  phase: Phase; lang: Lang; language: Language; onLang: (l: Lang) => void;
}) {
  const busy = phase === "starting" || phase === "stopping";
  const pending = phase === "running" ? language.pending : null;
  return (
    <div className="lang-picker">
      <span className="lang-label" id="lang-label">Idioma falado</span>
      <div className="lang-options" role="radiogroup" aria-labelledby="lang-label">
        {LANGS.map((l) => (
          <button key={l} type="button" role="radio" aria-checked={lang === l}
            className={`lang-option ${lang === l ? "on" : ""} ${pending === l ? "pending" : ""}`}
            disabled={busy} onClick={() => onLang(l)}>
            {LANG_NAME[l]}
          </button>
        ))}
      </div>
      <span className="lang-hint" role="status">
        {pending ? preparingText(pending, language.progress) : `tradução para ${LANG_NAME[other(lang)].toLowerCase()}`}
      </span>
    </div>
  );
}

function Card({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="card" role="status">
      <div className="card-label">{label}</div>
      <div className="card-body">{children}</div>
    </div>
  );
}
