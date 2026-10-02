import { afterEach, describe, expect, it, vi } from "vitest";
import { focusEditor, focusTerminal } from "../focus.svelte";
import { IS_MAC, installKeyboardRouter, type KeyboardAction } from "../keyboard";

describe("split shortcuts while an editor has focus", () => {
  let cleanup: () => void = () => {};

  afterEach(() => {
    cleanup();
    focusTerminal();
  });

  function pressInEditor(init: KeyboardEventInit): KeyboardAction[] {
    const onAction = vi.fn((_action: KeyboardAction) => {});
    cleanup = installKeyboardRouter(onAction, undefined, () => true);
    focusEditor();
    const modKey = IS_MAC ? "metaKey" : "ctrlKey";
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { bubbles: true, cancelable: true, [modKey]: true, ...init }),
    );
    return onAction.mock.calls.map(([action]) => action);
  }

  it.each([
    ["ArrowLeft", "move_tab_left"],
    ["ArrowRight", "move_tab_right"],
    ["ArrowUp", "move_tab_up"],
    ["ArrowDown", "move_tab_down"],
  ])("moves the tab with Mod+Alt+%s", (key, type) => {
    expect(pressInEditor({ key, altKey: true })).toEqual([{ type }]);
  });

  it("focuses a neighbor pane with Mod+Shift+Arrow", () => {
    // Mod is Ctrl off macOS, so these chords must not also require Ctrl to be up.
    expect(pressInEditor({ key: "ArrowLeft", shiftKey: true })).toEqual([
      { type: "focus_split_left" },
    ]);
  });

  it("still splits with Mod+D", () => {
    expect(pressInEditor({ key: "d" })).toEqual([{ type: "split_vertical" }]);
  });

  it("leaves shortcuts the editor owns to the editor", () => {
    expect(pressInEditor({ key: "b" })).toEqual([]);
  });
});
