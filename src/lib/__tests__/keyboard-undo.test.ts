import { afterEach, describe, expect, it, vi } from "vitest";
import { IS_MAC, installKeyboardRouter, type KeyboardAction } from "../keyboard";

describe("Mod+Z routing", () => {
  let cleanup: () => void = () => {};

  afterEach(() => {
    cleanup();
    document.body.innerHTML = "";
  });

  function pressModZIn(
    html: string,
    canUndo = true,
  ): { actions: KeyboardAction[]; prevented: boolean } {
    const onAction = vi.fn((_action: KeyboardAction) => {});
    const none = undefined;
    cleanup = installKeyboardRouter(onAction, none, none, none, none, () => canUndo);
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

  const terminal = '<textarea class="xterm-helper-textarea"></textarea>';

  it.each([
    ["the page", ""],
    ["the terminal", terminal],
  ])("reverts the last task move from %s", (_where, html) => {
    expect(pressModZIn(html)).toEqual({ actions: [{ type: "undo" }], prevented: true });
  });

  it("leaves the key to the terminal when there is no task move to revert", () => {
    expect(pressModZIn(terminal, false)).toEqual({ actions: [], prevented: false });
  });

  it.each([
    ["an input", "<input />"],
    ["a textarea", "<textarea></textarea>"],
    ["editable content", '<div contenteditable="true"></div>'],
  ])("leaves %s its own undo", (_where, html) => {
    expect(pressModZIn(html)).toEqual({ actions: [], prevented: false });
  });
});
