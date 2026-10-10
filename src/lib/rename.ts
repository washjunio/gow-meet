/** Teclas do campo de nome (janela flutuante e vozes guardadas): Enter salva, Esc cancela,
 *  nome vazio cancela. */
export type NameKeyAction = { action: "save"; name: string } | { action: "cancel" } | { action: "none" };

export const MAX_NAME = 60;

export function nameKey(key: string, draft: string): NameKeyAction {
  if (key === "Escape") return { action: "cancel" };
  if (key !== "Enter") return { action: "none" };
  const name = draft.trim();
  return name ? { action: "save", name } : { action: "cancel" };
}
