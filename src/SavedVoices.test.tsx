import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SavedVoices, type SavedVoicesProps } from "./SavedVoices";

const noop = () => {};
const base: SavedVoicesProps = {
  voices: [], error: null, locked: false, onRename: noop, onDelete: noop, onDeleteAll: noop,
};
const html = (p: Partial<SavedVoicesProps>) => renderToStaticMarkup(<SavedVoices {...base} {...p} />);
const deleteAll = (markup: string) => markup.match(/<button[^>]*>Apagar todas<\/button>/g) ?? [];

describe("SavedVoices", () => {
  it("mostra \"Apagar todas\" mesmo sem vozes na lista (arquivo corrompido ou só cópias preservadas)", () => {
    const m = html({ error: "o arquivo de vozes guardadas está corrompido" });
    expect(deleteAll(m)).toHaveLength(1);
    expect(deleteAll(m)[0]).not.toContain("disabled");
    expect(deleteAll(html({}))).toHaveLength(1);
  });

  it("\"Apagar todas\" fica desabilitado durante a reunião", () => {
    const [b] = deleteAll(html({ locked: true }));
    expect(b).toContain("disabled");
  });

  it("mantém a confirmação antes de apagar tudo", () => {
    const m = html({ initial: { editing: null, draft: "", confirm: "all" } });
    expect(m).toContain("Apagar todas as vozes?");
    expect(m).toContain("Cancelar");
  });
});
