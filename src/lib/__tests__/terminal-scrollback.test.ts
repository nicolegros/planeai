import { describe, expect, it } from "vitest";
import { parseScrollbackLines } from "../terminal-scrollback";

describe("parseScrollbackLines", () => {
  it("keeps values in range", () => {
    expect(parseScrollbackLines("50000")).toBe(50_000);
  });

  it("clamps values that would exhaust memory or overflow the config", () => {
    expect(parseScrollbackLines("1")).toBe(1_000);
    expect(parseScrollbackLines("5000000000")).toBe(1_000_000);
  });

  it("falls back to the default for anything that is not a number", () => {
    expect(parseScrollbackLines("")).toBeNull();
    expect(parseScrollbackLines("lots")).toBeNull();
  });
});
