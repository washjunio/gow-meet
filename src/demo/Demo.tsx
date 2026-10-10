/**
 * Modo de pré-visualização SÓ PARA DESENVOLVIMENTO: `npx vite` + `?demo=<estado>` no navegador,
 * fora do Tauri. Renderiza as telas com dados fictícios para conferir o visual.
 * Carregado por import dinâmico atrás de `import.meta.env.DEV`: fica fora do build de produção.
 */
import { MainView, type MainViewProps, type Session } from "../MainWindow";
import { OverlayView } from "../Overlay";
import { applyEvent, initial, type State, type UiEvent } from "../lib/transcript";

const noop = () => {};
const now = Date.parse("2026-10-08T15:00:00-03:00");
const min = 60_000;

const sessions: Session[] = [
  { id: "c", title: "Revisão do cronograma", started: now, ended: now + 47 * min,
    transcript: "/demo/c/transcript.md" },
  { id: "b", title: "Weekly sync com o time de produto", started: now - 26 * 60 * min,
    ended: now - 26 * 60 * min + 72 * min, transcript: "/demo/b/transcript.md" },
  { id: "a", title: "Reunião 06/10/2026 09:30", started: now - 3 * 24 * 60 * min,
    ended: now - 3 * 24 * 60 * min + 18 * min, transcript: "/demo/a/transcript.md" },
];

const baseMain: MainViewProps = {
  phase: "idle", title: "", onTitle: noop, onStart: noop, onStop: noop, progress: [],
  error: null, notice: null, permissions: {}, onOpenSettings: noop, lastTranscript: null,
  sessions, onOpen: noop,
};

const mainStates: Record<string, MainViewProps> = {
  "main-idle": { ...baseMain, lastTranscript: "/demo/c/transcript.md" },
  "main-starting": {
    ...baseMain, phase: "starting", title: "Planejamento da sprint",
    progress: [
      { file: "ggml-large-v3-turbo-q5_0.bin", done: 412e6, total: 574e6 },
      { file: "opus-mt-en-pt/model.bin", done: 61e6, total: 152e6 },
      { file: "opus-mt-en-pt/source.spm", done: 0.3e6, total: null },
    ],
  },
  "main-running": {
    ...baseMain, phase: "running", title: "Planejamento da sprint",
    sessions: [{ id: "d", title: "Planejamento da sprint", started: now + 2 * 60 * min, ended: null,
      transcript: null }, ...sessions],
    permissions: { screen: "Sem permissão de Gravação de Tela e Áudio do Sistema: a fala dos participantes não será capturada." },
  },
};

const ev: UiEvent[] = [
  { kind: "final", line: { id: "s0", t0: 0, t1: 2800, speaker: { source: "system", label: "Participante" },
    text: "Let's review the project timeline for next week.",
    translation: "Vamos revisar o cronograma do projeto para a próxima semana." } },
  { kind: "final", line: { id: "m0", t0: 3200, t1: 5400, speaker: { source: "mic", label: "Você" },
    text: "Sure, I can share the updated plan.",
    translation: "Claro, posso compartilhar o plano atualizado." } },
  { kind: "partial", id: "s1", speaker: { source: "system", label: "Participante" }, t0: 6000,
    text: "We still need to confirm the testing", translation: "Ainda precisamos confirmar os testes" },
  { kind: "lag", ms: 6000 },
];
const overlayStates: Record<string, State> = {
  "overlay": ev.reduce(applyEvent, initial),
  "overlay-empty": initial,
};

const demoStates = [...Object.keys(mainStates), ...Object.keys(overlayStates)];

export function Demo({ name }: { name: string }) {
  if (name in overlayStates) {
    // Janela de 440×320 sobre fundo cinza, para a transparência do painel aparecer.
    return (
      <div style={{ minHeight: "100vh", background: "#7a7a7a", padding: 40 }}>
        <div style={{ width: 440, height: 320 }}><OverlayView state={overlayStates[name]} /></div>
      </div>
    );
  }
  const props = mainStates[name];
  if (!props) return <p style={{ padding: 24 }}>Estados: {demoStates.join(", ")}</p>;
  return <MainView {...props} />;
}
