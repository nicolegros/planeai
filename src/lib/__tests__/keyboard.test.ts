import { describe, it, expect } from "vitest";
import { matchChord, IS_MAC, MOD_LABEL, MOD_ENTER_HINT, isPlatformMod } from "../keyboard";

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

describe("MOD_LABEL", () => {
  it("returns Ctrl+ on non-mac (jsdom)", () => {
    expect(IS_MAC).toBe(false);
    expect(MOD_LABEL).toBe("Ctrl+");
  });

  it("MOD_ENTER_HINT returns Ctrl+↵ on non-mac", () => {
    expect(MOD_ENTER_HINT).toBe("Ctrl+↵");
  });
});

describe("isPlatformMod", () => {
  it("returns true for ctrlKey on non-mac", () => {
    expect(isPlatformMod(key({ ctrlKey: true }))).toBe(true);
  });

  it("returns false for metaKey on non-mac", () => {
    expect(isPlatformMod(key({ metaKey: true }))).toBe(false);
  });

  it("returns false with no modifiers", () => {
    expect(isPlatformMod(key({}))).toBe(false);
  });
});

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
    expect(matchChord(key({ key: "Tab", ctrlKey: true, shiftKey: true }))).toEqual({
      type: "tab_switch_reverse",
    });
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

  it("returns new_tab on platform mod+T", () => {
    expect(matchChord(key({ key: "t", [modKey]: true }))).toEqual({
      type: "new_tab",
    });
  });

  it("returns close_tab on platform mod+W", () => {
    expect(matchChord(key({ key: "w", [modKey]: true }))).toEqual({
      type: "close_tab",
    });
  });

  it("returns next_tab on platform mod+]", () => {
    expect(matchChord(key({ key: "]", [modKey]: true }))).toEqual({
      type: "next_tab",
    });
  });

  it("returns prev_tab on platform mod+[", () => {
    expect(matchChord(key({ key: "[", [modKey]: true }))).toEqual({
      type: "prev_tab",
    });
  });

  it("returns next_session on platform mod+Shift+}", () => {
    expect(matchChord(key({ key: "]", [modKey]: true, shiftKey: true }))).toEqual({
      type: "next_session",
    });
  });

  it("returns prev_session on platform mod+Shift+{", () => {
    expect(matchChord(key({ key: "[", [modKey]: true, shiftKey: true }))).toEqual({
      type: "prev_session",
    });
  });

  it("returns toggle_sessions_panel on platform mod+Shift+S", () => {
    expect(matchChord(key({ key: "s", [modKey]: true, shiftKey: true }))).toEqual({
      type: "toggle_sessions_panel",
    });
  });

  it("returns toggle_task_panel on platform mod+Shift+T", () => {
    expect(matchChord(key({ key: "t", [modKey]: true, shiftKey: true }))).toEqual({
      type: "toggle_task_panel",
    });
  });
});
