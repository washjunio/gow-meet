import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { PermissionKind } from "./lib/transcript";
import { resetTranscript, useGowMeet } from "./lib/useGowMeet";

type Meta = { id: string; title: string; started: number; ended: number | null };
type Progress = { file: string; done: number; total: number | null };
type Phase = "idle" | "starting" | "running" | "stopping";

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

const mb = (n: number) => Math.round(n / 1e6);

export function MainWindow() {
  const state = useGowMeet();
  const [title, setTitle] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [sessions, setSessions] = useState<Meta[]>([]);

  const refresh = () => invoke<Meta[]>("list_sessions").then(setSessions).catch(() => {});
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

  const files = Object.values(progress);
  const permissionKinds = Object.keys(state.permissions) as PermissionKind[];

  return (
    <main className="main">
      <h1>gow-meet</h1>
      <p className="consent">Antes de começar, avise os participantes de que a reunião será transcrita.</p>
      <div className="row">
        <input placeholder="Título da reunião (opcional)" value={title}
          disabled={phase !== "idle"} onChange={(e) => setTitle(e.target.value)} />
        <button disabled={phase !== "idle"} onClick={start}>
          {phase === "starting" ? "Preparando…" : "Iniciar"}
        </button>
        <button disabled={phase !== "running"} onClick={stop}>
          {phase === "stopping" ? "Encerrando…" : "Encerrar"}
        </button>
      </div>
      <p className="hint-main">
        Use fones de ouvido: no alto-falante, a fala do participante também entra pelo microfone e
        aparece duplicada como &apos;Você&apos;.
      </p>
      {phase === "starting" && files.length > 0 && (
        <ul className="progress">
          {files.map((p) => (
            <li key={p.file}>
              {p.file}: {mb(p.done)} MB{p.total ? ` de ${mb(p.total)} MB` : ""}
              {p.total ? ` (${Math.round((p.done / p.total) * 100)}%)` : ""}
            </li>
          ))}
        </ul>
      )}
      {error && <p className="error">{error}</p>}
      {state.notice && <p className="error">{state.notice}</p>}
      {permissionKinds.map((k) => (
        <div key={k} className="permission">
          <p className="error">{state.permissions[k]}</p>
          <p>Habilite o gow-meet em Ajustes do Sistema → Privacidade e Segurança e inicie de novo.</p>
          <button onClick={() => openUrl(SETTINGS[k].url).catch((e) => setError(String(e)))}>
            {SETTINGS[k].label}
          </button>
        </div>
      ))}
      {state.transcript && phase === "idle" && (
        <button onClick={() => openPath(state.transcript!).catch((e) => setError(String(e)))}>
          Abrir última transcrição
        </button>
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
