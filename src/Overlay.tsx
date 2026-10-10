import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import symbol from "./assets/brand/gow-simbolo-negativo.png";
import { IconPencil } from "./icons";
import { MAX_NAME, nameKey } from "./lib/rename";
import { overlayStatus } from "./lib/status";
import { lastEntries, type Speaker, type State } from "./lib/transcript";
import { useGowMeet } from "./lib/useGowMeet";

type Rename = (voice: string, name: string) => Promise<unknown>;

export function Overlay() {
  return (
    <OverlayView state={useGowMeet()}
      onRename={(voice, name) => invoke<string>("rename_voice", { voice, name })} />
  );
}

/** Apresentação da janela flutuante (usada também no modo demo). O painel arrasta; só o
 *  rótulo de quem fala (sem `data-tauri-drag-region`) é clicável. `editing`: só para o demo. */
export function OverlayView({ state, onRename, editing }: { state: State; onRename?: Rename; editing?: string }) {
  const [renameError, setRenameError] = useState<string | null>(null);
  const entries = lastEntries(state, 3);
  const hasPermissionIssue = Object.keys(state.permissions).length > 0;
  const status = overlayStatus({
    permissionIssue: hasPermissionIssue, renameError, lagMs: state.lagMs, notice: state.notice,
  });
  // O erro de nome é temporário e não passa para a reunião seguinte (estado zerado).
  useEffect(() => {
    if (!renameError) return;
    const t = setTimeout(() => setRenameError(null), 5000);
    return () => clearTimeout(t);
  }, [renameError]);
  useEffect(() => {
    if (state.order.length === 0) setRenameError(null);
  }, [state.order.length]);
  const rename = onRename && ((voice: string, name: string) => {
    setRenameError(null);
    return onRename(voice, name).catch((e) => setRenameError(`Nome não salvo: ${String(e)}`));
  });
  return (
    <div className="overlay" data-tauri-drag-region>
      <div className="overlay-head" data-tauri-drag-region>
        <img className="overlay-symbol" src={symbol} alt="" draggable={false} data-tauri-drag-region />
        {entries.length > 0 && (
          <span className="overlay-live" data-tauri-drag-region><span className="dot" />Ao vivo</span>
        )}
        {status.text && (
          <span className={`overlay-status ${status.warn ? "lag" : ""}`} role="status" aria-live="polite"
            data-tauri-drag-region>
            {status.text}
          </span>
        )}
      </div>
      <div className="captions" data-tauri-drag-region>
        {entries.length === 0 && <p className="waiting" data-tauri-drag-region>Aguardando fala…</p>}
        {entries.map((e) => (
          <div key={e.id} className={`caption ${e.final ? "" : "partial"}`} data-tauri-drag-region>
            <SpeakerLabel speaker={e.speaker} onRename={rename}
              startEditing={editing !== undefined && editing === e.speaker.voice} />
            <div className="translation" data-tauri-drag-region>
              {e.translation ?? (e.translationError ? "(sem tradução)" : "…")}
            </div>
            <div className="original" data-tauri-drag-region>{e.text}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** "VOCÊ" em amarelo; participantes em cinza claro. Falas do sistema com grupo de voz
 *  viram botão: clique abre o campo no lugar do rótulo (Enter salva; Esc ou vazio cancela). */
function SpeakerLabel({ speaker, onRename, startEditing = false }:
  { speaker: Speaker; onRename?: Rename; startEditing?: boolean }) {
  const [editing, setEditing] = useState(startEditing);
  const [draft, setDraft] = useState("");
  if (speaker.source === "mic") return <div className="speaker me" data-tauri-drag-region>{speaker.label}</div>;
  const voice = speaker.voice;
  if (!voice || !onRename) return <div className="speaker" data-tauri-drag-region>{speaker.label}</div>;
  if (editing) {
    return (
      <input className="speaker-input" autoFocus value={draft} maxLength={MAX_NAME}
        placeholder={speaker.label} aria-label="Nome de quem fala"
        onChange={(ev) => setDraft(ev.target.value)}
        onBlur={() => setEditing(false)}
        onKeyDown={(ev) => {
          const r = nameKey(ev.key, draft);
          if (r.action === "none") return;
          ev.preventDefault();
          setEditing(false);
          if (r.action === "save") void onRename(voice, r.name);
        }} />
    );
  }
  return (
    <button type="button" className="speaker speaker-edit" title="Dar nome a esta voz"
      onClick={() => { setDraft(""); setEditing(true); }}>
      {speaker.label}<IconPencil />
    </button>
  );
}
