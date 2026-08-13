import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { IS_MAC, installKeyboardRouter, type KeyboardAction } from "../keyboard";

describe("installKeyboardRouter shortcut ownership", () => {
  let onAction: (action: KeyboardAction) => void;
  let cleanup: () => void;

  beforeEach(() => {
    onAction = vi.fn((_action: KeyboardAction) => {});
    cleanup = installKeyboardRouter(onAction);
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = "";
  });

  function addShortcutConsumer(): HTMLElement {
    const consumer = document.createElement("div");
    consumer.addEventListener("keydown", (event) => {
      event.preventDefault();
      event.stopPropagation();
    });
    document.body.appendChild(consumer);
    return consumer;
  }

  it.each([
    { action: { type: "tab_switch" }, name: "Ctrl+Tab", shiftKey: false },
    { action: { type: "tab_switch_reverse" }, name: "Ctrl+Shift+Tab", shiftKey: true },
  ])("routes $name before a focused consumer can claim it", ({ action, shiftKey }) => {
    const consumer = addShortcutConsumer();
    const event = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      key: "Tab",
      shiftKey,
    });

    consumer.dispatchEvent(event);

    expect(onAction).toHaveBeenCalledWith(action);
    expect(event.defaultPrevented).toBe(true);
  });

  it("lets a focused consumer claim non-reserved shortcuts", () => {
    const consumer = addShortcutConsumer();
    const modKey = IS_MAC ? "metaKey" : "ctrlKey";
    const event = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "b",
      [modKey]: true,
    });

    consumer.dispatchEvent(event);

    expect(onAction).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(true);
  });
});
