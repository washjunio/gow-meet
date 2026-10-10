import React from "react";
import ReactDOM from "react-dom/client";
import { MainWindow } from "./MainWindow";
import { Overlay } from "./Overlay";
import "./styles.css";

const root = ReactDOM.createRoot(document.getElementById("root")!);
const demo = new URLSearchParams(window.location.search).get("demo");

// Pré-visualização com dados fictícios, só em `vite` de desenvolvimento e fora do Tauri.
// `import.meta.env.DEV` vira `false` no build: o ramo e o módulo demo saem do bundle.
if (import.meta.env.DEV && demo !== null && !("__TAURI_INTERNALS__" in window)) {
  import("./demo/Demo").then(({ Demo }) =>
    root.render(<React.StrictMode><Demo name={demo} /></React.StrictMode>));
} else {
  const isOverlay = window.location.hash === "#overlay";
  document.documentElement.classList.toggle("is-overlay", isOverlay);
  root.render(<React.StrictMode>{isOverlay ? <Overlay /> : <MainWindow />}</React.StrictMode>);
}
