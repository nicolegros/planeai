import { describe, expect, it } from "vitest";
import { parseTooltipLines } from "../plugin-tooltip";

describe("parseTooltipLines", () => {
  it("keeps text and known tones", () => {
    expect(
      parseTooltipLines([
        { text: "#12 Fix login", tone: "success" },
        { text: "  #13 Stack base  " },
        { text: "#14 Draft", tone: "rainbow" },
      ]),
    ).toEqual([
      { text: "#12 Fix login", tone: "success" },
      { text: "#13 Stack base" },
      { text: "#14 Draft" },
    ]);
  });

  it("clears with an empty list and skips blank lines", () => {
    expect(parseTooltipLines([])).toEqual([]);
    expect(parseTooltipLines([{ text: "   " }])).toEqual([]);
  });

  it("caps the line count and the line length", () => {
    const lines = parseTooltipLines(Array.from({ length: 20 }, () => ({ text: "x".repeat(300) })))!;
    expect(lines).toHaveLength(12);
    expect(lines[0].text).toHaveLength(200);
  });

  it("rejects malformed messages", () => {
    expect(parseTooltipLines(undefined)).toBeNull();
    expect(parseTooltipLines("#12")).toBeNull();
    expect(parseTooltipLines([{ text: 12 }])).toBeNull();
    expect(parseTooltipLines([null])).toBeNull();
  });
});
