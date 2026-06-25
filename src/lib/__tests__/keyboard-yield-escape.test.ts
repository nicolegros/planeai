import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { installKeyboardRouter } from "../keyboard";
import { setActiveZone } from "../focus.svelte";

describe("installKeyboardRouter Escape yielding with data-form-keyboard", () => {
  let onAction: ReturnType<typeof vi.fn>;
  let cleanup: () => void;

  beforeEach(() => {
    onAction = vi.fn();
    setActiveZone("sidebar"); // not terminal, so shouldPassEscape path isn't triggered
    cleanup = installKeyboardRouter(
      onAction,
      () => true,
      () => false,
      () => !!document.activeElement?.closest("[data-form-keyboard]"),
    );
  });

  afterEach(() => {
    cleanup();
    document.body.innerHTML = "";
  });

  it("yields Escape when focus is inside a [data-form-keyboard] element", () => {
    const wrapper = document.createElement("div");
    wrapper.setAttribute("data-form-keyboard", "");
    wrapper.tabIndex = -1;
    const input = document.createElement("input");
    wrapper.appendChild(input);
    document.body.appendChild(wrapper);
    input.focus();

    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true });
    window.dispatchEvent(event);

    expect(onAction).not.toHaveBeenCalled();
  });

  it("does NOT yield Escape when focus is outside [data-form-keyboard]", () => {
    const outsideInput = document.createElement("input");
    document.body.appendChild(outsideInput);
    outsideInput.focus();

    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true });
    window.dispatchEvent(event);

    expect(onAction).toHaveBeenCalledWith({ type: "focus_terminal" });
  });
});
