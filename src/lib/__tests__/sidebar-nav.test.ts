import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  getSelectedIndex,
  setSelectedIndex,
  clampIndex,
  handleSidebarKey,
} from "../sidebar-nav.svelte";

function key(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "",
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    ...overrides,
  } as unknown as KeyboardEvent;
}

describe("sidebar-nav", () => {
  beforeEach(() => {
    setSelectedIndex(0);
  });

  describe("navigation", () => {
    it("ArrowDown increments index", () => {
      handleSidebarKey(key({ key: "ArrowDown" }), 5);
      expect(getSelectedIndex()).toBe(1);
    });

    it("ArrowUp decrements index", () => {
      setSelectedIndex(3);
      handleSidebarKey(key({ key: "ArrowUp" }), 5);
      expect(getSelectedIndex()).toBe(2);
    });

    it("j moves down", () => {
      handleSidebarKey(key({ key: "j" }), 5);
      expect(getSelectedIndex()).toBe(1);
    });

    it("k moves up", () => {
      setSelectedIndex(2);
      handleSidebarKey(key({ key: "k" }), 5);
      expect(getSelectedIndex()).toBe(1);
    });

    it("does not go below 0", () => {
      setSelectedIndex(0);
      handleSidebarKey(key({ key: "ArrowUp" }), 5);
      expect(getSelectedIndex()).toBe(0);
    });

    it("does not exceed list length", () => {
      setSelectedIndex(4);
      handleSidebarKey(key({ key: "ArrowDown" }), 5);
      expect(getSelectedIndex()).toBe(4);
    });
  });

  describe("actions", () => {
    it("Enter returns select", () => {
      const action = handleSidebarKey(key({ key: "Enter" }), 5);
      expect(action).toEqual({ type: "select" });
    });

    it("h returns collapse", () => {
      const action = handleSidebarKey(key({ key: "h" }), 5);
      expect(action).toEqual({ type: "collapse" });
    });

    it("ArrowLeft returns collapse", () => {
      const action = handleSidebarKey(key({ key: "ArrowLeft" }), 5);
      expect(action).toEqual({ type: "collapse" });
    });

    it("l returns expand", () => {
      const action = handleSidebarKey(key({ key: "l" }), 5);
      expect(action).toEqual({ type: "expand" });
    });

    it("ArrowRight returns expand", () => {
      const action = handleSidebarKey(key({ key: "ArrowRight" }), 5);
      expect(action).toEqual({ type: "expand" });
    });

    it("a returns archive", () => {
      const action = handleSidebarKey(key({ key: "a" }), 5);
      expect(action).toEqual({ type: "archive" });
    });

    it("r returns review", () => {
      const action = handleSidebarKey(key({ key: "r" }), 5);
      expect(action).toEqual({ type: "review" });
    });

    it("E returns rename", () => {
      const action = handleSidebarKey(key({ key: "E" }), 5);
      expect(action).toEqual({ type: "rename" });
    });

    it("R returns restart", () => {
      const action = handleSidebarKey(key({ key: "R" }), 5);
      expect(action).toEqual({ type: "restart" });
    });

    it("e returns edit", () => {
      const action = handleSidebarKey(key({ key: "e" }), 5);
      expect(action).toEqual({ type: "edit" });
    });

    it("dd returns delete", () => {
      const first = handleSidebarKey(key({ key: "d" }), 5);
      expect(first).toBeNull();
      const second = handleSidebarKey(key({ key: "d" }), 5);
      expect(second).toEqual({ type: "delete" });
    });
  });

  describe("status shortcuts", () => {
    it("st returns status todo", () => {
      handleSidebarKey(key({ key: "s" }), 5);
      const action = handleSidebarKey(key({ key: "t" }), 5);
      expect(action).toEqual({ type: "status", status: "todo" });
    });

    it("sp returns status in_progress", () => {
      handleSidebarKey(key({ key: "s" }), 5);
      const action = handleSidebarKey(key({ key: "p" }), 5);
      expect(action).toEqual({ type: "status", status: "in_progress" });
    });

    it("sr returns status in_review", () => {
      handleSidebarKey(key({ key: "s" }), 5);
      const action = handleSidebarKey(key({ key: "r" }), 5);
      expect(action).toEqual({ type: "status", status: "in_review" });
    });

    it("sd returns status done", () => {
      handleSidebarKey(key({ key: "s" }), 5);
      const action = handleSidebarKey(key({ key: "d" }), 5);
      expect(action).toEqual({ type: "status", status: "done" });
    });

    it("ss returns start_session", () => {
      handleSidebarKey(key({ key: "s" }), 5);
      const action = handleSidebarKey(key({ key: "s" }), 5);
      expect(action).toEqual({ type: "start_session" });
    });
  });

  describe("clampIndex", () => {
    it("clamps to list bounds", () => {
      setSelectedIndex(10);
      clampIndex(3);
      expect(getSelectedIndex()).toBe(2);
    });

    it("stays at 0 for empty list", () => {
      setSelectedIndex(5);
      clampIndex(0);
      expect(getSelectedIndex()).toBe(0);
    });
  });

  describe("empty list", () => {
    it("returns null for any key on empty list", () => {
      expect(handleSidebarKey(key({ key: "Enter" }), 0)).toBeNull();
      expect(handleSidebarKey(key({ key: "j" }), 0)).toBeNull();
    });
  });
});
