/** Data e hora no padrão brasileiro: "09/10/2026, 14:30". */
export const formatDate = (ms: number) =>
  new Date(ms).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });

/** Duração entre início e fim: "menos de 1 min", "42 min", "1 h 05 min". */
export function formatDuration(started: number, ended: number): string {
  const min = Math.floor(Math.max(0, ended - started) / 60000);
  if (min < 1) return "menos de 1 min";
  if (min < 60) return `${min} min`;
  return `${Math.floor(min / 60)} h ${String(min % 60).padStart(2, "0")} min`;
}

/** Percentual inteiro de um download; `null` quando o tamanho total é desconhecido. */
export const percent = (done: number, total: number | null) =>
  total ? Math.min(100, Math.round((done / total) * 100)) : null;
