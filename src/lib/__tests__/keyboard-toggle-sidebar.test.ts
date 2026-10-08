import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { installKeyboardRouter, IS_MAC, type KeyboardAction } from "../keyboard";
import { setActiveZone, getActiveZone, focusTerminal } from "../focus.svelte";

describe("installKeyboardRouter toggle_sidebar focus behavior", () => {
  let onAction: (action: KeyboardAction) => void;
  let cleanup: () => void;

  beforeEach(() => {
    onAction = vi.fn((_action: KeyboardAction) => {});
    focusTerminal();
    cleanup = installKeyboardRouter(
      onAction,
      () => true,
      () => false,
      () => false,
    );
  });

  afterEach(() => {
    cleanup();
  });

  it("does not change focus zone when toggle_sidebar fires from terminal zone", () => {
    setActiveZone("terminal");

    const modProp = IS_MAC ? "metaKey" : "ctrlKey";
    const event = new KeyboardEvent("keydown", {
      key: "b",
      [modProp]: true,
      bubbles: true,
    });
    window.dispatchEvent(event);

    expect(onAction).toHaveBeenCalledWith({ type: "toggle_sidebar" });
    // Focus zone must remain "terminal" — the keyboard router should NOT
    // toggle focus when handling a visibility action. App.svelte manages focus.
    expect(getActiveZone()).toBe("terminal");
  });

  it("does not change focus zone when toggle_sidebar fires from sidebar zone", () => {
    setActiveZone("sidebar");

    const modProp = IS_MAC ? "metaKey" : "ctrlKey";
    const event = new KeyboardEvent("keydown", {
      key: "b",
      [modProp]: true,
      bubbles: true,
    });
    window.dispatchEvent(event);

    expect(onAction).toHaveBeenCalledWith({ type: "toggle_sidebar" });
    // Focus zone must remain "sidebar" — App.svelte will decide what to do
    expect(getActiveZone()).toBe("sidebar");
  });
});
