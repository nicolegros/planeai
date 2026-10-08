import { describe, it, expect, vi, beforeEach } from "vitest";
import { createFormKeyboardController } from "../form-keyboard.svelte";

function makeKey(key: string, extra: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    key,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    ...extra,
  } as unknown as KeyboardEvent;
}

describe("createFormKeyboardController", () => {
  let wrapper: HTMLDivElement;
  let titleInput: HTMLInputElement;
  let onDismiss: () => void;
  let toggleDraft: () => void;

  beforeEach(() => {
    wrapper = document.createElement("div");
    wrapper.tabIndex = -1;
    titleInput = document.createElement("input");
    titleInput.setAttribute("data-field", "title");
    wrapper.appendChild(titleInput);
    document.body.appendChild(wrapper);
    onDismiss = vi.fn(() => {});
    toggleDraft = vi.fn(() => {});
  });

  it("starts in normal mode", () => {
    const fk = createFormKeyboardController(() => [{ key: "t", ref: () => titleInput }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    expect(fk.mode).toBe("normal");
  });

  it("focuses field ref on mnemonic key in normal mode", () => {
    const fk = createFormKeyboardController(() => [{ key: "t", ref: () => titleInput }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    const e = makeKey("t");
    fk.handleKeydown(e);
    expect(e.preventDefault).toHaveBeenCalled();
    expect(document.activeElement).toBe(titleInput);
  });

  it("calls toggle function on toggle binding", () => {
    const fk = createFormKeyboardController(() => [{ key: "d", toggle: toggleDraft }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    fk.handleKeydown(makeKey("d"));
    expect(toggleDraft).toHaveBeenCalledTimes(1);
  });

  it("enters insert mode on focusin to text input", () => {
    const fk = createFormKeyboardController(() => [{ key: "t", ref: () => titleInput }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    fk.handleFocusin({ target: titleInput } as unknown as FocusEvent);
    expect(fk.mode).toBe("insert");
  });

  it("escape in insert mode returns to normal", () => {
    const fk = createFormKeyboardController(() => [{ key: "t", ref: () => titleInput }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    // Enter insert mode
    fk.handleFocusin({ target: titleInput } as unknown as FocusEvent);
    expect(fk.mode).toBe("insert");
    // Escape → normal
    fk.handleKeydown(makeKey("Escape"));
    expect(fk.mode).toBe("normal");
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("escape in normal mode calls onDismiss", () => {
    const fk = createFormKeyboardController(() => [{ key: "t", ref: () => titleInput }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    fk.handleKeydown(makeKey("Escape"));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("does not intercept Cmd+Enter (lets it bubble for submit)", () => {
    const fk = createFormKeyboardController(() => [{ key: "t", ref: () => titleInput }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    const e = makeKey("Enter", { metaKey: true });
    fk.handleKeydown(e);
    expect(e.preventDefault).not.toHaveBeenCalled();
  });

  it("calls shiftToggle when shift+key is pressed and shiftToggle is defined", () => {
    const toggle = vi.fn();
    const shiftToggle = vi.fn();
    const fk = createFormKeyboardController(() => [{ key: "p", toggle, shiftToggle }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    fk.handleKeydown(makeKey("P", { shiftKey: true }));
    expect(shiftToggle).toHaveBeenCalledTimes(1);
    expect(toggle).not.toHaveBeenCalled();
  });

  it("falls back to toggle when shift+key is pressed but no shiftToggle", () => {
    const toggle = vi.fn();
    const fk = createFormKeyboardController(() => [{ key: "p", toggle }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    fk.handleKeydown(makeKey("P", { shiftKey: true }));
    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("calls toggle (not shiftToggle) when key pressed without shift", () => {
    const toggle = vi.fn();
    const shiftToggle = vi.fn();
    const fk = createFormKeyboardController(() => [{ key: "p", toggle, shiftToggle }], {
      wrapper: () => wrapper,
      onDismiss,
    });
    fk.handleKeydown(makeKey("p"));
    expect(toggle).toHaveBeenCalledTimes(1);
    expect(shiftToggle).not.toHaveBeenCalled();
  });
});
