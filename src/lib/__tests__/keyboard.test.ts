import { describe, it, expect } from "vitest";
import { matchChord } from "../keyboard";

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

describe("matchChord", () => {
  it("returns focus_terminal on Escape", () => {
    expect(matchChord(key({ key: "Escape" }))).toEqual({
      type: "focus_terminal",
    });
  });

  it("returns toggle_sidebar on Cmd+B", () => {
    expect(matchChord(key({ key: "b", metaKey: true }))).toEqual({
      type: "toggle_sidebar",
    });
  });

  it("returns null on Ctrl+B (only Cmd+B works)", () => {
    expect(matchChord(key({ key: "b", ctrlKey: true }))).toBeNull();
  });

  it("returns new_session on Cmd+N", () => {
    expect(matchChord(key({ key: "n", metaKey: true }))).toEqual({
      type: "new_session",
    });
  });

  it("returns jump_to_session on Cmd+1 through Cmd+9", () => {
    for (let i = 1; i <= 9; i++) {
      expect(matchChord(key({ key: String(i), metaKey: true }))).toEqual({
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
});
