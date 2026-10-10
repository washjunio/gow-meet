/** Texto de status da janela flutuante. Permissão ausente vence tudo; o erro de renomear
 *  (temporário) vence atraso e aviso enquanto estiver ativo. */
export function overlayStatus(o: {
  permissionIssue: boolean; renameError: string | null; lagMs: number | null; notice: string | null;
}): { text: string | null; warn: boolean } {
  if (o.permissionIssue) return { text: "Falta permissão de áudio: veja a janela principal", warn: false };
  if (o.renameError) return { text: o.renameError, warn: true };
  if (o.lagMs) return { text: `Atraso de ${Math.round(o.lagMs / 1000)} s`, warn: true };
  return { text: o.notice, warn: false };
}

/** Erro de ação (renomear/apagar) fica até a próxima ação; o de carga só aparece sem ele. */
export const voicesMessage = (action: string | null, load: string | null): string | null => action ?? load;
