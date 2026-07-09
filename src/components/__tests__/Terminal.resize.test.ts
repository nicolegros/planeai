import { describe, it, expect, vi, beforeEach } from "vitest";

/**
 * Tests for the terminal resize-on-focus/visibility fix (PLA-221).
 *
 * The Terminal component relies on:
 *  1. A ResizeObserver for container size changes
 *  2. A visibility $effect that runs when the pane becomes visible
 *  3. A focused $effect that runs when the pane gains focus
 *
 * The bug: when a terminal pane becomes visible or gains focus, the PTY may have
 * stale dimensions because:
 *  - The ResizeObserver doesn't fire when only CSS `hidden` is toggled (no size change)
 *  - The `lastSentDims` deduplication prevents re-sending the same dims
 *  - Another window may have resized the pane while it was unfocused
 *
 * These tests validate the resize logic extracted from the component:
 *  - fitAndResize() always sends pty.resize when dims differ from lastSentDims
 *  - lastSentDims is reset on visibility loss (so next restore always sends)
 *  - Focus-gain triggers a conditional resize (only if dims actually changed)
 */

// ── Mocked terminal & fit addon ─────────────────────────────────────────

function createMockTerminal(rows = 24, cols = 80) {
  return { rows, cols, focus: vi.fn(), blur: vi.fn() };
}

function createMockFitAddon(term: { rows: number; cols: number }) {
  return {
    fit: vi.fn(() => {
      // Simulate fit updating term dimensions (no-op by default, tests can mutate)
    }),
    proposeDimensions: vi.fn(() => ({ rows: term.rows, cols: term.cols })),
  };
}

const mockPtyResize = vi.fn();

// ── Extracted resize logic (mirrors Terminal.svelte implementation) ──────

interface ResizeState {
  lastSentDims: { cols: number; rows: number } | null;
}

function fitAndResize(
  term: { rows: number; cols: number },
  fitAddon: { fit: () => void },
  sessionId: string,
  state: ResizeState,
  ptyResize: (id: string, rows: number, cols: number) => void,
  force = false
) {
  fitAddon.fit();
  const { rows, cols } = term;
  if (rows > 0 && cols > 0) {
    if (!force && state.lastSentDims?.cols === cols && state.lastSentDims?.rows === rows) return;
    state.lastSentDims = { cols, rows };
    ptyResize(sessionId, rows, cols);
  }
}

function onVisibilityRestore(
  term: { rows: number; cols: number },
  fitAddon: { fit: () => void },
  sessionId: string,
  state: ResizeState,
  ptyResize: (id: string, rows: number, cols: number) => void
) {
  // Mirrors the visibility $effect: force=true (sans rAF for testability)
  fitAndResize(term, fitAddon, sessionId, state, ptyResize, true);
}

function onVisibilityLost(state: ResizeState) {
  state.lastSentDims = null;
}

function onFocusGain(
  term: { rows: number; cols: number },
  fitAddon: { fit: () => void },
  sessionId: string,
  state: ResizeState,
  ptyResize: (id: string, rows: number, cols: number) => void
) {
  // Mirrors the focused $effect: force=false (dedup applies)
  fitAndResize(term, fitAddon, sessionId, state, ptyResize, false);
}

// ── Tests ────────────────────────────────────────────────────────────────

describe("Terminal resize-on-focus (PLA-221)", () => {
  let term: ReturnType<typeof createMockTerminal>;
  let fitAddon: ReturnType<typeof createMockFitAddon>;
  let state: ResizeState;

  beforeEach(() => {
    term = createMockTerminal(24, 80);
    fitAddon = createMockFitAddon(term);
    state = { lastSentDims: null };
    mockPtyResize.mockClear();
  });

  describe("fitAndResize (ResizeObserver path)", () => {
    it("sends pty.resize when lastSentDims is null", () => {
      fitAndResize(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 24, 80);
      expect(state.lastSentDims).toEqual({ cols: 80, rows: 24 });
    });

    it("deduplicates when dims unchanged", () => {
      state.lastSentDims = { cols: 80, rows: 24 };
      fitAndResize(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).not.toHaveBeenCalled();
    });

    it("sends pty.resize when dims changed", () => {
      state.lastSentDims = { cols: 80, rows: 24 };
      term.rows = 30;
      term.cols = 120;
      fitAndResize(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 30, 120);
    });

    it("does not send if rows or cols are zero", () => {
      term.rows = 0;
      term.cols = 0;
      fitAndResize(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).not.toHaveBeenCalled();
    });

    it("sends even with unchanged dims when force=true", () => {
      state.lastSentDims = { cols: 80, rows: 24 };
      fitAndResize(term, fitAddon, "s1", state, mockPtyResize, true);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 24, 80);
    });
  });

  describe("visibility $effect", () => {
    it("always sends pty.resize on visibility restore (unconditional)", () => {
      // Simulate: already had dims sent, then pane was hidden, now visible again
      state.lastSentDims = { cols: 80, rows: 24 };
      onVisibilityLost(state);
      expect(state.lastSentDims).toBeNull();

      onVisibilityRestore(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 24, 80);
    });

    it("sends even if terminal size has not changed from before hide", () => {
      // This is the key bug fix: PTY might have stale dims even though terminal
      // reports same size as before
      state.lastSentDims = { cols: 80, rows: 24 };
      onVisibilityLost(state);
      // Term dims unchanged
      onVisibilityRestore(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 24, 80);
    });

    it("sends correct dims when fit changes terminal size during restore", () => {
      // Simulate: window was resized while pane was hidden, fit corrects to new dims
      state.lastSentDims = { cols: 80, rows: 24 };
      onVisibilityLost(state);

      fitAddon.fit = vi.fn(() => {
        term.rows = 40;
        term.cols = 100;
      });

      onVisibilityRestore(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 40, 100);
      expect(state.lastSentDims).toEqual({ cols: 100, rows: 40 });
    });

    it("invalidates lastSentDims on visibility loss", () => {
      state.lastSentDims = { cols: 80, rows: 24 };
      onVisibilityLost(state);
      expect(state.lastSentDims).toBeNull();
    });
  });

  describe("focused $effect", () => {
    it("sends pty.resize when dims differ from lastSentDims on focus gain", () => {
      // Simulates: another window resized container while this pane was unfocused
      state.lastSentDims = { cols: 80, rows: 24 };
      fitAddon.fit = vi.fn(() => {
        term.rows = 30;
        term.cols = 120;
      });

      onFocusGain(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 30, 120);
    });

    it("does NOT send pty.resize when dims match lastSentDims (no wasted IPC)", () => {
      state.lastSentDims = { cols: 80, rows: 24 };
      onFocusGain(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).not.toHaveBeenCalled();
    });

    it("handles null lastSentDims (first focus after mount)", () => {
      state.lastSentDims = null;
      onFocusGain(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 24, 80);
    });
  });

  describe("two-instance scenario (make dev + real planeai)", () => {
    it("recovers correct dims after external resize while hidden", () => {
      // Initial state: terminal visible with known dims
      state.lastSentDims = { cols: 80, rows: 24 };

      // User switches to another app / other planeai instance resizes window
      onVisibilityLost(state);

      // Meanwhile, container was resized by the other instance
      fitAddon.fit = vi.fn(() => {
        term.rows = 50;
        term.cols = 200;
      });

      // User switches back — visibility restored
      onVisibilityRestore(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 50, 200);
      expect(state.lastSentDims).toEqual({ cols: 200, rows: 50 });
    });

    it("focus gain catches resize that happened between visibility restore and focus", () => {
      // Visibility just restored with correct dims
      state.lastSentDims = { cols: 80, rows: 24 };

      // But by the time focus effect runs, another layout shift happened
      fitAddon.fit = vi.fn(() => {
        term.rows = 25;
        term.cols = 80;
      });

      onFocusGain(term, fitAddon, "s1", state, mockPtyResize);
      expect(mockPtyResize).toHaveBeenCalledWith("s1", 25, 80);
    });
  });
});
