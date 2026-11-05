import { describe, expect, it } from "vitest";
import { selectorChordOutcome } from "../new-tab-selector";

describe("selectorChordOutcome", () => {
  it("keeps the selector open on Mod+T", () => {
    expect(selectorChordOutcome("new_tab")).toBe("keep");
  });

  it("only dismisses the selector on closing chords", () => {
    expect(selectorChordOutcome("close_tab")).toBe("dismiss-only");
    expect(selectorChordOutcome("close_split")).toBe("dismiss-only");
  });

  it("dismisses the selector and runs any other chord", () => {
    expect(selectorChordOutcome("command_palette")).toBe("dismiss");
    expect(selectorChordOutcome("new_task")).toBe("dismiss");
  });
});
