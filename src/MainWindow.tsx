import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState, type ReactNode } from "react";
import logo from "./assets/brand/gow-horizontal-negativo.png";
import { IconDocument, IconHeadphones, IconMegaphone } from "./icons";
import { formatDate, formatDuration, percent } from "./lib/format";
import type { PermissionKind } from "./lib/transcript";
import { resetTranscript, useGowMeet } from "./lib/useGowMeet";

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

export function MainWindow() {
  const state = useGowMeet();
  const [title, setTitle] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [sessions, setSessions] = useState<Session[]>([]);

  const refresh = () => invoke<Session[]>("list_sessions").then(setSessions).catch(() => {});
  useEffect(() => {
    refresh();
    const un = listen<Progress>("models-progress", (e) =>
      setProgress((p) => ({ ...p, [e.payload.file]: e.payload })));
    return () => { un.then((f) => f()); };
  }, []);

  async function start() {
    setPhase("starting"); setError(null); setProgress({});
    try {
      await invoke("ensure_models");
      await resetTranscript(); // nova sessão: ids s0/m0 voltam a existir
      await invoke("start_session", { title: title || null });
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
                  Habilite o gow-meet em Ajustes do Sistema → Privacidade e Segurança e inicie de novo.
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
        </main>
      </div>
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
