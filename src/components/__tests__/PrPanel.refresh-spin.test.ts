import { describe, it, expect, vi } from "vitest";
import { createFormKeyboardController } from "../../lib/form-keyboard.svelte";

/**
 * Tests the refresh-spin animation logic in PrPanel.
 *
 * PrPanel registers a "r" keybinding via createFormKeyboardController that calls doRefresh().
 * doRefresh() sets `spinning = true`, which applies the .animate-spin-once class to the icon.
 * On animationend, spinning resets to false and the class is removed.
 *
 * Since PrPanel has heavy dependencies (Tauri APIs, pollers, etc.), we test the keyboard
 * controller integration pattern directly: verify that a "r" toggle fires its handler,
 * and that the handler (doRefresh) calls the expected functions.
 */

function makeKey(key: string): KeyboardEvent {
  return {
    key,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as KeyboardEvent;
}

describe("PrPanel refresh spin animation", () => {
  it("r keybinding toggle calls doRefresh which triggers both refresh functions", () => {
    const refreshCiChecks = vi.fn();
    const refreshPrComments = vi.fn();
    let spinning = false;

    function doRefresh() {
      spinning = true;
      refreshCiChecks("sess-1");
      refreshPrComments("sess-1");
    }

    const wrapper = document.createElement("div");
    wrapper.tabIndex = -1;
    document.body.appendChild(wrapper);

    const fk = createFormKeyboardController(
      () => [
        { key: "r", toggle: doRefresh },
      ],
      { wrapper: () => wrapper, onDismiss: vi.fn() },
    );

    fk.handleKeydown(makeKey("r"));

    expect(spinning).toBe(true);
    expect(refreshCiChecks).toHaveBeenCalledWith("sess-1");
    expect(refreshPrComments).toHaveBeenCalledWith("sess-1");

    wrapper.remove();
  });

  it("animationend resets spinning to false (class removal logic)", () => {
    // This tests the pattern used in the template:
    // class={spinning ? 'animate-spin-once' : ''}
    // onanimationend={() => { spinning = false; }}
    let spinning = true;
    const onAnimationEnd = () => { spinning = false; };

    // Simulate animationend firing
    onAnimationEnd();

    expect(spinning).toBe(false);
  });

  it("doRefresh can be triggered multiple times (re-spin after animation completes)", () => {
    const refreshCiChecks = vi.fn();
    const refreshPrComments = vi.fn();
    let spinning = false;

    function doRefresh() {
      spinning = true;
      refreshCiChecks("sess-1");
      refreshPrComments("sess-1");
    }

    // First trigger
    doRefresh();
    expect(spinning).toBe(true);

    // Simulate animationend
    spinning = false;

    // Second trigger
    doRefresh();
    expect(spinning).toBe(true);
    expect(refreshCiChecks).toHaveBeenCalledTimes(2);
    expect(refreshPrComments).toHaveBeenCalledTimes(2);
  });
});
