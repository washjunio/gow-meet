import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { OverlayView } from "./Overlay";
import { applyEvent, initial, type UiEvent } from "./lib/transcript";

// Frases e nomes fictícios.
const sys = (id: string, voice: string, t0: number): UiEvent => ({
  kind: "final", line: { id, t0, t1: t0 + 900, speaker: { source: "system", label: "Participante", voice },
    text: `Fictional sentence ${id}.`, translation: `Frase fictícia ${id}.` },
});
const rename = () => Promise.resolve();

describe("OverlayView: campo de nome", () => {
  it("o campo e o texto digitado continuam na tela depois que chegam falas novas", () => {
    const before = [sys("s0", "v1", 0), sys("s1", "v2", 1000)].reduce(applyEvent, initial);
    const edit = { voice: "v1", entry: "s0", draft: "Fulana Fictícia" };
    expect(renderToStaticMarkup(<OverlayView state={before} onRename={rename} editing={edit} />))
      .toContain('value="Fulana Fictícia"');
    // Quatro falas novas: s0 sairia das 3 últimas, mas fica fixada com o campo aberto.
    const after = [sys("s2", "v2", 2000), sys("s3", "v2", 3000), sys("s4", "v2", 4000), sys("s5", "v2", 5000)]
      .reduce(applyEvent, before);
    const m = renderToStaticMarkup(<OverlayView state={after} onRename={rename} editing={edit} />);
    expect(m).toContain('value="Fulana Fictícia"');
    expect(m).toContain("Fictional sentence s0.");
    expect(m).toContain("Fictional sentence s5.");
    expect(m).not.toContain("Fictional sentence s3.");
    expect(m.match(/class="caption /g)).toHaveLength(3);
  });

  it("fala do campo que sumiu (reunião nova) não deixa campo aberto", () => {
    const m = renderToStaticMarkup(
      <OverlayView state={initial} onRename={rename} editing={{ voice: "v1", entry: "s0", draft: "x" }} />);
    expect(m).not.toContain("<input");
    expect(m).toContain("Aguardando fala");
  });
});
