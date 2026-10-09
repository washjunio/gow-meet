import React from "react";
import ReactDOM from "react-dom/client";
import { MainWindow } from "./MainWindow";
import { Overlay } from "./Overlay";
import "./styles.css";

const isOverlay = window.location.hash === "#overlay";
document.documentElement.classList.toggle("is-overlay", isOverlay);
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isOverlay ? <Overlay /> : <MainWindow />}</React.StrictMode>,
);
