import { describe, expect, it } from "vitest";
import { formatDuration, percent } from "./format";

describe("formatDuration", () => {
  it("formata minutos e horas", () => {
    expect(formatDuration(0, 30_000)).toBe("menos de 1 min");
    expect(formatDuration(0, 42 * 60_000 + 59_000)).toBe("42 min");
    expect(formatDuration(0, 65 * 60_000)).toBe("1 h 05 min");
    expect(formatDuration(10, 0)).toBe("menos de 1 min");
  });
});

describe("percent", () => {
  it("arredonda e limita a 100", () => {
    expect(percent(1, 3)).toBe(33);
    expect(percent(5, 4)).toBe(100);
    expect(percent(5, null)).toBeNull();
  });
});
