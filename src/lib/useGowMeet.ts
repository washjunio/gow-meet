import { emit, listen } from "@tauri-apps/api/event";
import { useEffect, useReducer } from "react";
import { applyEvent, initial, type UiEvent } from "./transcript";

const RESET = "gow-meet-reset";

/** Avisa todas as janelas (principal e flutuante) para limpar a transcrição. */
export const resetTranscript = () => emit(RESET);

export function useGowMeet() {
  const [state, dispatch] = useReducer(applyEvent, initial);
  useEffect(() => {
    const unlisten = [
      listen<UiEvent>("gow-meet", (e) => dispatch(e.payload)),
      listen(RESET, () => dispatch({ kind: "reset" })),
    ];
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
    };
  }, []);
  return state;
}
