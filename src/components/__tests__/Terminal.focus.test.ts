import { describe, it, expect, vi, beforeEach } from "vitest";

/**
 * Tests for the terminal focus-on-open fix (PLA-235).
 *
 * The bug: when creating a new tab, `term.focus()` was called before
 * `term.open()` completed (async font loading). The xterm.js textarea
 * only exists after `open()`, so focus was a no-op. The `opened` state
 * variable was not tracked by the focus $effect, so the effect never
 * re-ran after the terminal was ready.
 *
 * The fix: the focus $effect now guards on `opened` in addition to `term`.
 * When `opened` transitions to `true`, the effect re-runs and correctly
 * calls `term.focus()`.
 */

function createMockTerminal() {
  return {
    rows: 24,
    cols: 80,
    focus: vi.fn(),
    blur: vi.fn(),
  };
}

// ── Extracted focus logic (mirrors Terminal.svelte $effect) ───────────────

interface FocusState {
  term: ReturnType<typeof createMockTerminal> | null;
  opened: boolean;
  focused: boolean;
}

/**
 * Mirrors the focus $effect body from Terminal.svelte.
 * This is the FIXED version that guards on `opened`.
 */
function onFocusChange(state: FocusState, fitAndResize: () => void) {
  if (!state.term || !state.opened) return;
  if (state.focused) {
    state.term.focus();
    fitAndResize();
  } else {
    state.term.blur();
  }
}

// ── Tests ────────────────────────────────────────────────────────────────

describe("Terminal focus-on-open (PLA-235)", () => {
  let state: FocusState;
  let fitAndResize: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    state = {
      term: createMockTerminal(),
      opened: false,
      focused: true, // new tab starts focused
    };
    fitAndResize = vi.fn();
  });

  describe("opened guard prevents premature focus", () => {
    it("does NOT call term.focus() before terminal is opened", () => {
      // Effect runs with focused=true but opened=false (font still loading)
      onFocusChange(state, fitAndResize);

      expect(state.term!.focus).not.toHaveBeenCalled();
      expect(fitAndResize).not.toHaveBeenCalled();
    });

    it("calls term.focus() when opened transitions to true", () => {
      // First run: opened=false, focus deferred
      onFocusChange(state, fitAndResize);
      expect(state.term!.focus).not.toHaveBeenCalled();

      // Font loads, term.open() called, opened=true
      state.opened = true;

      // Effect re-runs because `opened` changed
      onFocusChange(state, fitAndResize);

      expect(state.term!.focus).toHaveBeenCalledTimes(1);
      expect(fitAndResize).toHaveBeenCalledTimes(1);
    });

    it("calls term.blur() when focused becomes false after open", () => {
      state.opened = true;
      state.focused = false;

      onFocusChange(state, fitAndResize);

      expect(state.term!.blur).toHaveBeenCalledTimes(1);
      expect(state.term!.focus).not.toHaveBeenCalled();
    });
  });

  describe("focus toggling after terminal is opened", () => {
    beforeEach(() => {
      state.opened = true;
    });

    it("focuses when focused=true and opened=true", () => {
      state.focused = true;
      onFocusChange(state, fitAndResize);

      expect(state.term!.focus).toHaveBeenCalledTimes(1);
    });

    it("blurs when focused transitions to false", () => {
      state.focused = false;
      onFocusChange(state, fitAndResize);

      expect(state.term!.blur).toHaveBeenCalledTimes(1);
    });

    it("re-focuses when focused toggles back to true", () => {
      state.focused = false;
      onFocusChange(state, fitAndResize);
      expect(state.term!.blur).toHaveBeenCalledTimes(1);

      state.focused = true;
      onFocusChange(state, fitAndResize);
      expect(state.term!.focus).toHaveBeenCalledTimes(1);
    });
  });

  describe("no term guard", () => {
    it("does nothing when term is null", () => {
      state.term = null;
      state.opened = true;
      state.focused = true;

      // Should not throw
      expect(() => onFocusChange(state, fitAndResize)).not.toThrow();
      expect(fitAndResize).not.toHaveBeenCalled();
    });
  });

  describe("new tab lifecycle simulation", () => {
    it("simulates the full new-tab lifecycle: mount → font load → focus", () => {
      // 1. Component mounts, term is created, focused=true, opened=false
      //    $effect runs — should NOT focus yet
      onFocusChange(state, fitAndResize);
      expect(state.term!.focus).not.toHaveBeenCalled();

      // 2. waitForFont resolves, term.open() is called, opened=true
      //    $effect re-runs because `opened` is now tracked
      state.opened = true;
      onFocusChange(state, fitAndResize);

      // 3. Terminal should now be focused
      expect(state.term!.focus).toHaveBeenCalledTimes(1);
      expect(fitAndResize).toHaveBeenCalledTimes(1);
    });
  });
});
