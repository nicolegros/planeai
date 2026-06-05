import { describe, it, expect } from "vitest";
import { matchTerminalKey } from "../terminal-keys";

function key(
  overrides: Partial<{
    key: string;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
  }>,
) {
  return { key: "", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...overrides };
}

// In jsdom, IS_MAC is false — these test the Windows/Linux branch.

describe("matchTerminalKey (Windows/Linux)", () => {
  describe("copy", () => {
    it("Ctrl+Shift+C with selection returns copy", () => {
      expect(matchTerminalKey(key({ key: "C", ctrlKey: true, shiftKey: true }), true)).toEqual({
        type: "copy",
      });
    });

    it("Ctrl+Shift+C without selection returns null", () => {
      expect(matchTerminalKey(key({ key: "C", ctrlKey: true, shiftKey: true }), false)).toBeNull();
    });

    it("Ctrl+C with selection returns copy", () => {
      expect(matchTerminalKey(key({ key: "c", ctrlKey: true }), true)).toEqual({ type: "copy" });
    });

    it("Ctrl+C without selection returns null (passes as interrupt)", () => {
      expect(matchTerminalKey(key({ key: "c", ctrlKey: true }), false)).toBeNull();
    });
  });

  describe("paste", () => {
    it("Ctrl+Shift+V returns paste", () => {
      expect(matchTerminalKey(key({ key: "V", ctrlKey: true, shiftKey: true }), false)).toEqual({
        type: "paste",
      });
    });

    it("Ctrl+V returns passthrough", () => {
      expect(matchTerminalKey(key({ key: "v", ctrlKey: true }), false)).toEqual({
        type: "passthrough",
      });
    });
  });

  describe("line navigation", () => {
    it("Home sends Ctrl+A (beginning of line)", () => {
      expect(matchTerminalKey(key({ key: "Home" }), false)).toEqual({
        type: "send_bytes",
        bytes: [0x01],
      });
    });

    it("End sends Ctrl+E (end of line)", () => {
      expect(matchTerminalKey(key({ key: "End" }), false)).toEqual({
        type: "send_bytes",
        bytes: [0x05],
      });
    });

    it("Ctrl+Backspace sends Ctrl+U (kill line)", () => {
      expect(matchTerminalKey(key({ key: "Backspace", ctrlKey: true }), false)).toEqual({
        type: "send_bytes",
        bytes: [0x15],
      });
    });
  });
});
