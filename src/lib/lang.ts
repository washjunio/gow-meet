/** Idiomas falados (etapa 3). A tradução é sempre para o outro. */
export type Lang = "en" | "pt";
export const LANGS: Lang[] = ["en", "pt"];
export const LANG_NAME: Record<Lang, string> = { en: "Inglês", pt: "Português" };

/** Idioma da reunião: o que vale, o pedido em preparo e o progresso do download (0–1). */
export type Language = { active: Lang | null; pending: Lang | null; progress: number | null };

export const other = (l: Lang): Lang => (l === "en" ? "pt" : "en");
/** "EN → PT" */
export const pairLabel = (l: Lang) => `${l.toUpperCase()} → ${other(l).toUpperCase()}`;
/** Separador entre falas quando o idioma muda. */
export const switchedTo = (l: Lang) => `— agora em ${LANG_NAME[l].toLowerCase()} —`;
export const preparingText = (l: Lang, progress: number | null) =>
  `Preparando ${LANG_NAME[l].toLowerCase()}…${progress != null ? ` ${Math.round(progress * 100)}%` : ""}`;

/** Clique no botão da flutuante: troca para o outro idioma; com um preparo pendente, cancela
 *  (volta ao atual). `null` sem reunião. */
export function toggleTarget(l: Language): Lang | null {
  if (!l.active) return null;
  return l.pending ? l.active : other(l.active);
}

const KEY = "gow-meet-lang";

/** Última escolha da janela principal; sem armazenamento (ou bloqueado), inglês. */
export function loadLang(): Lang {
  try {
    return localStorage.getItem(KEY) === "pt" ? "pt" : "en";
  } catch {
    return "en";
  }
}

export function saveLang(l: Lang) {
  try {
    localStorage.setItem(KEY, l);
  } catch {
    // Sem armazenamento: só não lembra a escolha.
  }
}
