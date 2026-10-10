import { useState } from "react";
import { formatDate } from "./lib/format";
import { MAX_NAME, nameKey } from "./lib/rename";

/** Item de `list_voices`: nunca traz a impressão de voz. */
export type SavedVoice = { id: string; name: string; updated: number; count: number };

/** Estado local da seção; o modo demo passa um inicial para mostrar cada situação. */
export type VoicesUi = { editing: string | null; draft: string; confirm: "all" | string | null };
const idle: VoicesUi = { editing: null, draft: "", confirm: null };

export type SavedVoicesProps = {
  voices: SavedVoice[];
  error: string | null;
  /** Durante a reunião as vozes guardadas ficam só para leitura. */
  locked: boolean;
  onRename: (id: string, name: string) => void;
  onDelete: (id: string) => void;
  onDeleteAll: () => void;
  initial?: VoicesUi;
};

export const PRIVACY_TEXT =
  "Vozes ficam só neste computador. Apagar remove a impressão de voz; o consentimento é responsabilidade da gow.";

export function SavedVoices(p: SavedVoicesProps) {
  const [ui, setUi] = useState<VoicesUi>(p.initial ?? idle);
  const reset = () => setUi(idle);
  return (
    <section className="section">
      <div className="section-head">
        <h2 className="title">Vozes guardadas</h2>
        {/* Sempre visível: mesmo com a lista vazia (arquivo corrompido ou só cópias
            `voices.json.corrompido-*`) ainda pode haver impressões de voz no disco. */}
        {(ui.confirm === "all"
          ? (
            <span className="confirm">
              <span>Apagar todas as vozes?</span>
              <button className="btn btn-danger btn-small" disabled={p.locked} onClick={() => { reset(); p.onDeleteAll(); }}>Apagar todas</button>
              <button className="btn btn-ghost btn-small" onClick={reset}>Cancelar</button>
            </span>
          )
          : (
            <button className="btn btn-ghost btn-small" disabled={p.locked}
              onClick={() => setUi({ ...idle, confirm: "all" })}>Apagar todas</button>
          ))}
      </div>
      <p className="note privacy">{PRIVACY_TEXT}</p>
      {p.locked && p.voices.length > 0 && <p className="note">Encerre a reunião para alterar as vozes guardadas.</p>}
      {p.error && <p className="voices-error" role="status">{p.error}</p>}
      {p.voices.length === 0
        ? <p className="empty">Nenhuma voz guardada. Dê nome a uma voz na janela flutuante durante a reunião.</p>
        : (
          <ul className="voices">
            {p.voices.map((v) => (
              <li key={v.id} className="voice">
                {ui.editing === v.id
                  ? (
                    <input className="field field-small" autoFocus value={ui.draft} maxLength={MAX_NAME}
                      placeholder={v.name} aria-label={`Novo nome para ${v.name}`}
                      onChange={(e) => setUi({ ...ui, draft: e.target.value })}
                      onBlur={reset}
                      onKeyDown={(e) => {
                        const r = nameKey(e.key, ui.draft);
                        if (r.action === "none") return;
                        e.preventDefault();
                        reset();
                        if (r.action === "save") p.onRename(v.id, r.name);
                      }} />
                  )
                  : <span className="voice-name">{v.name}</span>}
                <span className="voice-seen">Vista em {formatDate(v.updated)}</span>
                <span className="voice-count">{v.count} {v.count === 1 ? "fala" : "falas"}</span>
                <span className="voice-actions">
                  {ui.confirm === v.id
                    ? (
                      <>
                        <button className="btn btn-danger btn-small" disabled={p.locked} aria-label={`Apagar ${v.name}`}
                          onClick={() => { reset(); p.onDelete(v.id); }}>Apagar</button>
                        <button className="btn btn-ghost btn-small" onClick={reset}>Cancelar</button>
                      </>
                    )
                    : (
                      <>
                        <button className="btn btn-ghost btn-small" disabled={p.locked} aria-label={`Renomear ${v.name}`}
                          onClick={() => setUi({ ...idle, editing: v.id })}>Renomear</button>
                        <button className="btn btn-ghost btn-small" disabled={p.locked} aria-label={`Apagar ${v.name}`}
                          onClick={() => setUi({ ...idle, confirm: v.id })}>Apagar</button>
                      </>
                    )}
                </span>
              </li>
            ))}
          </ul>
        )}
    </section>
  );
}
