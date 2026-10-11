import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MainView, type MainViewProps } from "./MainWindow";

const noop = () => {};
const base: MainViewProps = {
  phase: "idle", title: "", onTitle: noop, onStart: noop, onStop: noop, progress: [],
  error: null, notice: null, permissions: {}, onOpenSettings: noop, lastTranscript: null,
  sessions: [
    { id: "b", title: "Com transcrição", started: 2, ended: 3, transcript: "/d/b/transcript.md" },
    { id: "a", title: "Em andamento", started: 1, ended: null, transcript: null },
  ],
  onOpen: noop, onReveal: noop, voices: [], voicesError: null,
  onRenameVoice: noop, onDeleteVoice: noop, onDeleteAllVoices: noop,
  lang: "en", language: { active: null, pending: null, progress: null }, onLang: noop,
};

describe("MainView", () => {
  it("mostra a transcrição no Finder só nas reuniões que já têm o arquivo", () => {
    const m = renderToStaticMarkup(<MainView {...base} />);
    expect(m.match(/aria-label="Mostrar no Finder"/g)).toHaveLength(1);
  });
});
