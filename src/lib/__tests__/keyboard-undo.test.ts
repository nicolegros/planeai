import { afterEach, describe, expect, it, vi } from "vitest";
import { IS_MAC, installKeyboardRouter, type KeyboardAction } from "../keyboard";

describe("Mod+Z routing", () => {
  let cleanup: () => void = () => {};

  afterEach(() => {
    cleanup();
    document.body.innerHTML = "";
  });

  function pressModZIn(html: string): { actions: KeyboardAction[]; prevented: boolean } {
    const onAction = vi.fn((_action: KeyboardAction) => {});
    cleanup = installKeyboardRouter(onAction);
    document.body.innerHTML = html;
    const event = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "z",
      [IS_MAC ? "metaKey" : "ctrlKey"]: true,
    });
    (document.body.firstElementChild ?? document.body).dispatchEvent(event);
    return {
      actions: onAction.mock.calls.map(([action]) => action),
      prevented: event.defaultPrevented,
    };
  }

  it.each([
    ["the page", ""],
    ["the terminal", '<textarea class="xterm-helper-textarea"></textarea>'],
  ])("reverts the last task move from %s", (_where, html) => {
    expect(pressModZIn(html)).toEqual({ actions: [{ type: "undo" }], prevented: true });
  });

  it.each([
    ["an input", "<input />"],
    ["a textarea", "<textarea></textarea>"],
    ["editable content", '<div contenteditable="true"></div>'],
  ])("leaves %s its own undo", (_where, html) => {
    expect(pressModZIn(html)).toEqual({ actions: [], prevented: false });
  });
});
