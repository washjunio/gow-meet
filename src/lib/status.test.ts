import { describe, expect, it } from "vitest";
import { overlayStatus, voicesMessage } from "./status";

const base = { permissionIssue: false, renameError: null, lagMs: null, notice: null };

describe("overlayStatus", () => {
  it("permissão vence tudo", () => {
    expect(overlayStatus({ ...base, permissionIssue: true, renameError: "x" }).text).toMatch(/permissão/);
  });
  it("erro de nome vence atraso; sem ele o atraso e o aviso voltam", () => {
    expect(overlayStatus({ ...base, renameError: "x", lagMs: 6000, notice: "n" })).toEqual({ text: "x", warn: true });
    expect(overlayStatus({ ...base, lagMs: 6000, notice: "n" })).toEqual({ text: "Atraso de 6 s", warn: true });
    expect(overlayStatus({ ...base, notice: "Reconectando áudio…" })).toEqual({ text: "Reconectando áudio…", warn: false });
  });
});

describe("voicesMessage", () => {
  it("erro de ação não é apagado pela carga", () => {
    expect(voicesMessage("Encerre a reunião", null)).toBe("Encerre a reunião");
    expect(voicesMessage(null, "corrompido")).toBe("corrompido");
  });
});
