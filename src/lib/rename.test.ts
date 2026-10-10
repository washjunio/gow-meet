import { describe, expect, it } from "vitest";
import { nameKey } from "./rename";

describe("nameKey", () => {
  it("Enter salva o nome aparado", () => {
    expect(nameKey("Enter", "  Ana  ")).toEqual({ action: "save", name: "Ana" });
  });
  it("Esc cancela, mesmo com texto", () => {
    expect(nameKey("Escape", "Ana")).toEqual({ action: "cancel" });
  });
  it("Enter com nome vazio cancela", () => {
    expect(nameKey("Enter", "   ")).toEqual({ action: "cancel" });
    expect(nameKey("Enter", "")).toEqual({ action: "cancel" });
  });
  it("outras teclas seguem editando", () => {
    expect(nameKey("a", "An")).toEqual({ action: "none" });
    expect(nameKey("Tab", "Ana")).toEqual({ action: "none" });
  });
});
