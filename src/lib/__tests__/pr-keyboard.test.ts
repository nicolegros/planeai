import { describe, it, expect, vi } from "vitest";
import { matchChord, IS_MAC } from "../keyboard";

const modKey = IS_MAC ? "metaKey" : "ctrlKey";

function key(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "",
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("toggle_pr_panel shortcut", () => {
  it("returns toggle_pr_panel on Mod+Shift+P", () => {
    expect(matchChord(key({ key: "p", [modKey]: true, shiftKey: true }))).toEqual({
      type: "toggle_pr_panel",
    });
  });

  it("does not conflict with open_file (Mod+P without shift)", () => {
    expect(matchChord(key({ key: "p", [modKey]: true }))).toEqual({
      type: "open_file",
    });
  });
});
