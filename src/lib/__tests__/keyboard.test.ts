import { describe, it, expect } from "vitest";
import { matchChord, IS_MAC } from "../keyboard";

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

// In test environment (jsdom), navigator.platform is empty so IS_MAC is false.
// Platform mod is ctrlKey in tests.
const modKey = IS_MAC ? "metaKey" : "ctrlKey";

describe("matchChord", () => {
  it("returns focus_terminal on Escape", () => {
    expect(matchChord(key({ key: "Escape" }))).toEqual({
      type: "focus_terminal",
    });
  });

  it("returns toggle_sidebar on platform mod+B", () => {
    expect(matchChord(key({ key: "b", [modKey]: true }))).toEqual({
      type: "toggle_sidebar",
    });
  });

  it("returns null when wrong modifier is used", () => {
    const wrongMod = IS_MAC ? "ctrlKey" : "metaKey";
    expect(matchChord(key({ key: "b", [wrongMod]: true }))).toBeNull();
  });

  it("returns new_session on platform mod+N", () => {
    expect(matchChord(key({ key: "n", [modKey]: true }))).toEqual({
      type: "new_session",
    });
  });

  it("returns new_project on platform mod+Shift+N", () => {
    expect(matchChord(key({ key: "n", [modKey]: true, shiftKey: true }))).toEqual({
      type: "new_project",
    });
  });

  it("returns jump_to_session on platform mod+1 through mod+9", () => {
    for (let i = 1; i <= 9; i++) {
      expect(matchChord(key({ key: String(i), [modKey]: true }))).toEqual({
        type: "jump_to_session",
        index: i - 1,
      });
    }
  });

  it("returns tab_switch on Ctrl+Tab", () => {
    expect(matchChord(key({ key: "Tab", ctrlKey: true }))).toEqual({
      type: "tab_switch",
    });
  });

  it("returns tab_switch_reverse on Ctrl+Shift+Tab", () => {
    expect(
      matchChord(key({ key: "Tab", ctrlKey: true, shiftKey: true })),
    ).toEqual({ type: "tab_switch_reverse" });
  });

  it("returns null for unmatched keys", () => {
    expect(matchChord(key({ key: "a" }))).toBeNull();
    expect(matchChord(key({ key: "Enter" }))).toBeNull();
    expect(matchChord(key({ key: "b" }))).toBeNull(); // no modifier
  });

  it("returns command_palette on platform mod+K", () => {
    expect(matchChord(key({ key: "k", [modKey]: true }))).toEqual({
      type: "command_palette",
    });
  });
});
