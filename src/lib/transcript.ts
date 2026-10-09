export type Speaker = { source: "mic" | "system"; label: string };
export type Line = {
  id: string; t0: number; t1: number; speaker: Speaker;
  text: string; translation?: string | null; translation_error?: boolean;
};
/** Eventos `"gow-meet"` emitidos pelo backend (campo `kind`). */
export type UiEvent =
  | { kind: "partial"; id: string; speaker: Speaker; t0: number; text: string; translation?: string | null }
  | { kind: "final"; line: Line }
  | { kind: "translated"; line: Line }
  | { kind: "dropped"; id: string }
  | { kind: "lag"; ms: number }
  | { kind: "error"; message: string }
  | { kind: "reconnecting"; source: "mic" | "system" }
  | { kind: "stopped"; transcript: string };
/** `reset` é local do front-end: emitido ao iniciar uma nova sessão. */
export type Action = UiEvent | { kind: "reset" };

export type PermissionKind = "screen" | "mic";

export type Entry = {
  id: string; speaker: Speaker; t0: number; text: string;
  translation: string | null; final: boolean; translationError: boolean;
};
export type State = {
  order: string[]; byId: Record<string, Entry>;
  lagMs: number | null; notice: string | null; transcript: string | null;
  /** Avisos de permissão ausente: persistem até a sessão parar. */
  permissions: Partial<Record<PermissionKind, string>>;
};

export const initial: State = { order: [], byId: {}, lagMs: null, notice: null, transcript: null, permissions: {} };

/** Reconhece avisos sobre permissão ausente de Microfone ou de Gravação de Tela e Áudio do Sistema. */
export function permissionKind(message: string): PermissionKind | null {
  if (!/permiss/i.test(message)) return null;
  if (/microfone/i.test(message)) return "mic";
  if (/tela|áudio do sistema/i.test(message)) return "screen";
  return null;
}

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

export function applyEvent(s: State, ev: Action): State {
  switch (ev.kind) {
    case "reset":
      return initial;
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
      return upsert(s, { ...fromLine(ev.line, s.byId[ev.line.id]), translation: ev.line.translation ?? null });
    case "dropped": {
      if (!s.byId[ev.id]) return s;
      const byId = { ...s.byId };
      delete byId[ev.id];
      return { ...s, byId, order: s.order.filter((id) => id !== ev.id) };
    }
    case "lag":
      return { ...s, lagMs: ev.ms };
    case "error": {
      const kind = permissionKind(ev.message);
      if (kind) return { ...s, permissions: { ...s.permissions, [kind]: ev.message } };
      return { ...s, notice: ev.message };
    }
    case "reconnecting":
      return { ...s, notice: "Reconectando áudio…" };
    case "stopped":
      return { ...s, transcript: ev.transcript, permissions: {} };
  }
}

export function lastEntries(s: State, n = 3): Entry[] {
  return s.order.slice(-n).map((id) => s.byId[id]);
}
