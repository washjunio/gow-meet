import symbol from "./assets/brand/gow-simbolo-negativo.png";
import { lastEntries, type State } from "./lib/transcript";
import { useGowMeet } from "./lib/useGowMeet";

export function Overlay() {
  return <OverlayView state={useGowMeet()} />;
}

/** Apresentação da janela flutuante (usada também no modo demo). O painel inteiro arrasta. */
export function OverlayView({ state }: { state: State }) {
  const entries = lastEntries(state, 3);
  const hasPermissionIssue = Object.keys(state.permissions).length > 0;
  const status = hasPermissionIssue
    ? "Falta permissão de áudio: veja a janela principal"
    : state.lagMs ? `Atraso de ${Math.round(state.lagMs / 1000)} s` : state.notice;
  return (
    <div className="overlay" data-tauri-drag-region>
      <div className="overlay-head" data-tauri-drag-region>
        <img className="overlay-symbol" src={symbol} alt="" draggable={false} data-tauri-drag-region />
        {entries.length > 0 && (
          <span className="overlay-live" data-tauri-drag-region><span className="dot" />Ao vivo</span>
        )}
        {status && (
          <span className={`overlay-status ${!hasPermissionIssue && state.lagMs ? "lag" : ""}`}
            data-tauri-drag-region>
            {status}
          </span>
        )}
      </div>
      <div className="captions" data-tauri-drag-region>
        {entries.length === 0 && <p className="waiting" data-tauri-drag-region>Aguardando fala…</p>}
        {entries.map((e) => (
          <div key={e.id} className={`caption ${e.final ? "" : "partial"}`} data-tauri-drag-region>
            <div className={`speaker ${e.speaker.source === "mic" ? "me" : ""}`} data-tauri-drag-region>
              {e.speaker.label}
            </div>
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
