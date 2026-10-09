import { lastEntries } from "./lib/transcript";
import { useGowMeet } from "./lib/useGowMeet";

export function Overlay() {
  const state = useGowMeet();
  const entries = lastEntries(state, 3);
  const hasPermissionIssue = Object.keys(state.permissions).length > 0;
  return (
    <div className="overlay" data-tauri-drag-region>
      {entries.length === 0 && <p className="hint" data-tauri-drag-region>Aguardando fala…</p>}
      {entries.map((e) => (
        <div key={e.id} className={`bubble ${e.final ? "" : "partial"}`} data-tauri-drag-region>
          <div className="speaker" data-tauri-drag-region>{e.speaker.label}</div>
          <div className="translation" data-tauri-drag-region>
            {e.translation ?? (e.translationError ? "(sem tradução)" : "…")}
          </div>
          <div className="original" data-tauri-drag-region>{e.text}</div>
        </div>
      ))}
      {(state.lagMs || state.notice || hasPermissionIssue) && (
        <div className="status" data-tauri-drag-region>
          {hasPermissionIssue
            ? "Falta permissão de áudio: veja a janela principal"
            : state.lagMs ? `Atraso de ${Math.round(state.lagMs / 1000)} s` : state.notice}
        </div>
      )}
    </div>
  );
}
