import { describe, expect, it } from "vitest";
import { applyEvent, initial, lastEntries, permissionKind, visibleEntries, type Action, type Line } from "./transcript";

const sys = { source: "system" as const, label: "Participante" };
const line = (id: string, t0: number, text: string, translation: string | null | undefined, err = false): Line =>
  ({ id, t0, t1: t0 + 900, speaker: sys, text, translation, translation_error: err });
const run = (...evs: Action[]) => evs.reduce(applyEvent, initial);

describe("applyEvent", () => {
  it("partial → final → translated mantém uma entrada pelo id", () => {
    const s = run(
      { kind: "partial", id: "s0", speaker: sys, t0: 0, text: "Hello", translation: null },
      { kind: "final", line: line("s0", 0, "Hello team.", null) },
      { kind: "translated", line: line("s0", 0, "Hello team.", "Olá, equipe.") },
    );
    expect(s.order).toEqual(["s0"]);
    expect(s.byId.s0).toMatchObject({ final: true, text: "Hello team.", translation: "Olá, equipe." });
  });

  it("parcial atrasada não reabre fala fechada", () => {
    const s = run(
      { kind: "final", line: line("s0", 0, "Hello team.", null) },
      { kind: "partial", id: "s0", speaker: sys, t0: 0, text: "Hel", translation: null },
    );
    expect(s.byId.s0).toMatchObject({ final: true, text: "Hello team." });
  });

  it("falha de tradução fica marcada", () => {
    const s = run({ kind: "translated", line: line("s0", 0, "Hello team.", null, true) });
    expect(s.byId.s0.translationError).toBe(true);
  });

  it("tradução ausente (undefined) é tratada como nula", () => {
    const s = run({ kind: "final", line: line("s0", 0, "Hello team.", undefined) });
    expect(s.byId.s0.translation).toBeNull();
  });

  it("ordena por t0 e devolve as últimas n", () => {
    const s = run(
      { kind: "final", line: line("s1", 2000, "B", null) },
      { kind: "final", line: line("m0", 1000, "A", null) },
      { kind: "final", line: line("s2", 3000, "C", null) },
      { kind: "final", line: line("s3", 4000, "D", null) },
    );
    expect(lastEntries(s, 3).map((e) => e.id)).toEqual(["s1", "s2", "s3"]);
  });

  it("avisos de atraso e reconexão somem na próxima fala fechada", () => {
    let s = run({ kind: "lag", ms: 6000 }, { kind: "reconnecting", source: "mic" });
    expect(s.lagMs).toBe(6000);
    expect(s.notice).toBe("Reconectando áudio…");
    s = applyEvent(s, { kind: "final", line: line("s0", 0, "Hi.", null) });
    expect(s.lagMs).toBeNull();
    expect(s.notice).toBeNull();
  });

  it("stopped guarda o caminho da transcrição", () => {
    expect(run({ kind: "stopped", transcript: "/tmp/x/transcript.md" }).transcript).toBe("/tmp/x/transcript.md");
  });

  it("dropped remove a parcial ou fala com o id", () => {
    const s = run(
      { kind: "partial", id: "s0", speaker: sys, t0: 0, text: "um", translation: null },
      { kind: "final", line: line("s1", 1000, "Two.", null) },
      { kind: "dropped", id: "s0" },
      { kind: "dropped", id: "s1" },
      { kind: "dropped", id: "nao-existe" },
    );
    expect(s.order).toEqual([]);
    expect(s.byId).toEqual({});
  });

  it("reset ao iniciar nova sessão evita colisão de ids", () => {
    let s = run(
      { kind: "final", line: line("s0", 0, "First meeting.", "Primeira reunião.") },
      { kind: "stopped", transcript: "/tmp/a.md" },
    );
    s = applyEvent(s, { kind: "reset" });
    expect(s).toEqual(initial);
    s = applyEvent(s, { kind: "partial", id: "s0", speaker: sys, t0: 0, text: "Second", translation: null });
    expect(s.byId.s0).toMatchObject({ final: false, text: "Second", translation: null });
  });

  it("aviso de permissão persiste após falas e some ao parar", () => {
    const msg = "sem permissão de Gravação de Tela e Áudio do Sistema (x)";
    let s = run({ kind: "error", message: msg }, { kind: "final", line: line("s0", 0, "Hi.", null) });
    expect(s.permissions).toEqual({ screen: msg });
    expect(s.notice).toBeNull();
    s = applyEvent(s, { kind: "error", message: "sem permissão de microfone" });
    expect(Object.keys(s.permissions).sort()).toEqual(["mic", "screen"]);
    s = applyEvent(s, { kind: "stopped", transcript: "/tmp/x.md" });
    expect(s.permissions).toEqual({});
  });

  it("evento speaker troca o rótulo de todas as falas do grupo", () => {
    const v1 = { source: "system" as const, label: "Participante 1", voice: "v1" };
    const v2 = { source: "system" as const, label: "Participante 2", voice: "v2" };
    const me = { source: "mic" as const, label: "Você" };
    const s = run(
      { kind: "final", line: { ...line("s0", 0, "Hello.", null), speaker: v1 } },
      { kind: "final", line: { ...line("s1", 1000, "Hi.", null), speaker: v2 } },
      { kind: "final", line: { ...line("m0", 1500, "Hey.", null), speaker: me } },
      { kind: "final", line: { ...line("s2", 2000, "Next topic.", null), speaker: v1 } },
      { kind: "speaker", voice: "v1", label: "Ana" },
    );
    expect(s.order.map((id) => s.byId[id].speaker.label)).toEqual(["Ana", "Participante 2", "Você", "Ana"]);
    expect(s.byId.s0.speaker).toEqual({ source: "system", label: "Ana", voice: "v1" });
  });

  it("tradução que chega depois do nome mantém o nome", () => {
    const v1 = { source: "system" as const, label: "Participante 1", voice: "v1" };
    const s = run(
      { kind: "final", line: { ...line("s0", 0, "Hello.", null), speaker: v1 } },
      { kind: "speaker", voice: "v1", label: "Ana" },
      { kind: "translated", line: { ...line("s0", 0, "Hello.", "Olá."), speaker: v1 } },
      { kind: "final", line: { ...line("s1", 900, "Bye.", null), speaker: v1 } },
    );
    expect(s.byId.s0).toMatchObject({ translation: "Olá.", speaker: { label: "Ana" } });
    expect(s.byId.s1.speaker.label).toBe("Ana");
  });

  it("reset esquece os nomes da reunião anterior", () => {
    const s = run({ kind: "speaker", voice: "v1", label: "Ana" }, { kind: "reset" });
    expect(s.labels).toEqual({});
  });

  it("classifica avisos de permissão", () => {
    expect(permissionKind("sem permissão de Gravação de Tela e Áudio do Sistema (x)")).toBe("screen");
    expect(permissionKind("Sem permissão para o Microfone")).toBe("mic");
    expect(permissionKind("nenhum microfone")).toBeNull();
    // aviso do backend quando o microfone só entrega zeros exatos (acesso negado)
    expect(permissionKind(
      "O microfone só entrega silêncio absoluto: verifique a permissão de Microfone do gow-meet.",
    )).toBe("mic");
    expect(permissionKind("GPU indisponível")).toBeNull();
  });
});

describe("visibleEntries", () => {
  const four = run(...["s0", "s1", "s2", "s3"].map((id, i) => ({ kind: "final", line: line(id, i * 1000, "Hi.", null) }) as Action));
  it("sem fala fixada é igual a lastEntries", () => {
    expect(visibleEntries(four, 3).map((e) => e.id)).toEqual(["s1", "s2", "s3"]);
    expect(visibleEntries(four, 3, null).map((e) => e.id)).toEqual(["s1", "s2", "s3"]);
  });
  it("a fala com o campo de nome aberto continua na tela quando chegam falas novas", () => {
    expect(visibleEntries(four, 3, "s1").map((e) => e.id)).toEqual(["s1", "s2", "s3"]);
    const more = run(...["s0", "s1", "s2", "s3", "s4", "s5"]
      .map((id, i) => ({ kind: "final", line: line(id, i * 1000, "Hi.", null) }) as Action));
    expect(visibleEntries(more, 3, "s1").map((e) => e.id)).toEqual(["s1", "s4", "s5"]);
  });
  it("fala fixada que sumiu (descartada ou reunião nova) é ignorada", () => {
    expect(visibleEntries(four, 3, "x9").map((e) => e.id)).toEqual(["s1", "s2", "s3"]);
    expect(visibleEntries(initial, 3, "s1")).toEqual([]);
  });
});
