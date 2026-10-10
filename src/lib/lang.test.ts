import { afterEach, describe, expect, it, vi } from "vitest";
import { loadLang, other, pairLabel, saveLang, switchedTo, toggleTarget } from "./lang";

function fakeStorage() {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
}

afterEach(() => vi.unstubAllGlobals());

describe("lang", () => {
  it("textos e o outro idioma", () => {
    expect(other("en")).toBe("pt");
    expect(pairLabel("pt")).toBe("PT → EN");
    expect(switchedTo("pt")).toBe("— agora em português —");
  });

  it("botão da flutuante: troca para o outro; com preparo pendente, volta ao atual", () => {
    expect(toggleTarget({ active: "en", pending: null, progress: null })).toBe("pt");
    expect(toggleTarget({ active: "en", pending: "pt", progress: 0.4 })).toBe("en");
    expect(toggleTarget({ active: null, pending: null, progress: null })).toBeNull();
  });

  it("lembra a última escolha; sem armazenamento, inglês", () => {
    vi.stubGlobal("localStorage", fakeStorage());
    expect(loadLang()).toBe("en");
    saveLang("pt");
    expect(loadLang()).toBe("pt");
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("bloqueado"); }, setItem: () => { throw new Error("bloqueado"); } });
    expect(loadLang()).toBe("en");
    expect(() => saveLang("pt")).not.toThrow();
  });
});
