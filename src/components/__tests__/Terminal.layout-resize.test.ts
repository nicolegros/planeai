import { describe, it, expect, vi, beforeEach } from "vitest";

/**
 * Tests for the terminal resize-during-layout-transition fix.
 *
 * The bug: when switching sessions to one with a saved multi-pane layout,
 * `deserialize()` replaces the entire tree, creating new DOM containers.
 * The Terminal's visibility $effect fires and calls fitAndResize, but the
 * container may not have received its CSS layout yet (percentage-based widths
 * from the split tree). This caused fitAddon.fit() to compute wrong
 * cols/rows, sending incorrect dimensions to the PTY and producing shifted/
 * clipped terminal content.
 *
 * The fix:
 * 1. fitAndResize bails out when container has zero dimensions
 * 2. The visibility $effect retries fitAndResize across animation frames
 *    until the container has non-zero dimensions (up to MAX_FIT_RETRIES)
 * 3. loadLayoutForSession awaits tick() after deserialize to ensure Svelte
 *    flushes DOM before terminals attempt to fit
 */

interface MockContainer {
  clientWidth: number;
  clientHeight: number;
}

interface MockTerm {
  rows: number;
  cols: number;
  focus: () => void;
}

interface FitAndResizeState {
  containerEl: MockContainer | null;
  fitAddon: { fit: () => void };
  term: MockTerm;
  lastSentDims: { cols: number; rows: number } | null;
  resizeCalls: Array<{ rows: number; cols: number }>;
}

/**
 * Mirrors the fitAndResize logic from Terminal.svelte (FIXED version).
 * Includes the zero-dimension guard via hasUsableDimensions.
 */
function hasUsableDimensions(containerEl: MockContainer | null): boolean {
  if (!containerEl) return false;
  return containerEl.clientWidth > 0 && containerEl.clientHeight > 0;
}

function fitAndResize(state: FitAndResizeState, force = false) {
  // Guard: skip fitting when the container has no usable dimensions.
  if (!hasUsableDimensions(state.containerEl)) {
    return;
  }
  state.fitAddon.fit();
  const { rows, cols } = state.term;
  if (rows > 0 && cols > 0) {
    if (!force && state.lastSentDims?.cols === cols && state.lastSentDims?.rows === rows) return;
    state.lastSentDims = { cols, rows };
    state.resizeCalls.push({ rows, cols });
  }
}

/**
 * Mirrors the visibility-restore retry logic from Terminal.svelte.
 * Returns the number of attempts needed to successfully fit.
 */
function simulateVisibilityRestore(
  state: FitAndResizeState,
  containerDimensionsByFrame: Array<{ width: number; height: number }>,
  maxRetries = 5,
): { attempts: number; fitSucceeded: boolean } {
  let attempts = 0;
  const MAX_FIT_RETRIES = maxRetries;

  function attemptFit(): boolean {
    if (!state.containerEl) return false;
    if (!hasUsableDimensions(state.containerEl)) {
      if (++attempts < MAX_FIT_RETRIES) {
        // Simulate next frame: update container dimensions
        const nextDims = containerDimensionsByFrame[attempts];
        if (nextDims) {
          state.containerEl.clientWidth = nextDims.width;
          state.containerEl.clientHeight = nextDims.height;
        }
        return attemptFit(); // recursive to simulate rAF chain
      }
      return false;
    }
    fitAndResize(state, true);
    return true;
  }

  // Simulate initial double-rAF: set first frame dimensions
  if (containerDimensionsByFrame.length > 0) {
    state.containerEl!.clientWidth = containerDimensionsByFrame[0].width;
    state.containerEl!.clientHeight = containerDimensionsByFrame[0].height;
  }

  const fitSucceeded = attemptFit();
  return { attempts, fitSucceeded };
}

// ── Tests ────────────────────────────────────────────────────────────────

describe("Terminal resize-during-layout-transition fix", () => {
  let state: FitAndResizeState;

  beforeEach(() => {
    state = {
      containerEl: { clientWidth: 800, clientHeight: 600 },
      fitAddon: { fit: vi.fn() },
      term: { rows: 24, cols: 80, focus: vi.fn() },
      lastSentDims: null,
      resizeCalls: [],
    };
  });

  describe("fitAndResize zero-dimension guard", () => {
    it("bails out when container has zero width", () => {
      state.containerEl = { clientWidth: 0, clientHeight: 600 };

      fitAndResize(state, true);

      expect(state.fitAddon.fit).not.toHaveBeenCalled();
      expect(state.resizeCalls).toHaveLength(0);
    });

    it("bails out when container has zero height", () => {
      state.containerEl = { clientWidth: 800, clientHeight: 0 };

      fitAndResize(state, true);

      expect(state.fitAddon.fit).not.toHaveBeenCalled();
      expect(state.resizeCalls).toHaveLength(0);
    });

    it("bails out when container has both zero dimensions", () => {
      state.containerEl = { clientWidth: 0, clientHeight: 0 };

      fitAndResize(state, true);

      expect(state.fitAddon.fit).not.toHaveBeenCalled();
      expect(state.resizeCalls).toHaveLength(0);
    });

    it("proceeds normally when container has valid dimensions", () => {
      state.containerEl = { clientWidth: 800, clientHeight: 600 };

      fitAndResize(state, true);

      expect(state.fitAddon.fit).toHaveBeenCalledTimes(1);
      expect(state.resizeCalls).toHaveLength(1);
      expect(state.resizeCalls[0]).toEqual({ rows: 24, cols: 80 });
    });

    it("bails out when containerEl is null (no container available)", () => {
      state.containerEl = null;

      fitAndResize(state, true);

      expect(state.fitAddon.fit).not.toHaveBeenCalled();
      expect(state.resizeCalls).toHaveLength(0);
    });

    it("deduplicates resize calls when not forced", () => {
      fitAndResize(state, true); // first call — sends resize
      fitAndResize(state, false); // second call — should dedup

      expect(state.resizeCalls).toHaveLength(1);
    });

    it("sends resize again after lastSentDims is cleared (visibility restore)", () => {
      fitAndResize(state, true);
      expect(state.resizeCalls).toHaveLength(1);

      // Simulating visibility restore: clear cached dims
      state.lastSentDims = null;
      fitAndResize(state, false);

      expect(state.resizeCalls).toHaveLength(2);
    });
  });

  describe("visibility-restore retry logic", () => {
    it("succeeds immediately when container has dimensions on first frame", () => {
      const result = simulateVisibilityRestore(state, [
        { width: 400, height: 600 }, // first frame: half-width (split pane)
      ]);

      expect(result.fitSucceeded).toBe(true);
      expect(result.attempts).toBe(0);
      expect(state.resizeCalls).toHaveLength(1);
    });

    it("retries when container starts with zero dimensions (tree just deserialized)", () => {
      const result = simulateVisibilityRestore(state, [
        { width: 0, height: 0 },     // frame 0: DOM exists but no layout yet
        { width: 0, height: 0 },     // frame 1: still waiting for CSS
        { width: 400, height: 600 }, // frame 2: split-child percentage applied
      ]);

      expect(result.fitSucceeded).toBe(true);
      expect(result.attempts).toBe(2); // needed 2 retries
      expect(state.resizeCalls).toHaveLength(1);
    });

    it("gives up after MAX_FIT_RETRIES if layout never stabilizes", () => {
      const result = simulateVisibilityRestore(
        state,
        [
          { width: 0, height: 0 },
          { width: 0, height: 0 },
          { width: 0, height: 0 },
          { width: 0, height: 0 },
          { width: 0, height: 0 },
        ],
        5,
      );

      expect(result.fitSucceeded).toBe(false);
      expect(state.resizeCalls).toHaveLength(0);
    });

    it("handles split-pane scenario: initially full width then narrows", () => {
      // This simulates the case where fitAddon.fit() would have computed
      // wrong dimensions at the full-width container before the split CSS applied
      state.term = { rows: 24, cols: 80, focus: vi.fn() }; // full width dims

      const result = simulateVisibilityRestore(state, [
        { width: 400, height: 600 }, // split-child percentage already applied
      ]);

      expect(result.fitSucceeded).toBe(true);
      expect(state.resizeCalls).toHaveLength(1);
    });
  });

  describe("session switch with multi-pane layout", () => {
    it("prevents stale resize when switching from single to split pane", () => {
      // Scenario: session A is single-pane (800px wide), session B has a 50/50 split
      // When switching to B, the tree is deserialized and containers go from
      // 800px -> 0px (new DOM) -> 400px (after CSS layout)

      // Step 1: terminal was visible at full width
      state.containerEl = { clientWidth: 800, clientHeight: 600 };
      fitAndResize(state, true);
      expect(state.resizeCalls[0]).toEqual({ rows: 24, cols: 80 });

      // Step 2: session switch — lastSentDims cleared (visibility=false)
      state.lastSentDims = null;

      // Step 3: new tree deserialized, container is now in a split-child
      // but CSS hasn't been applied yet (zero dimensions)
      state.containerEl = { clientWidth: 0, clientHeight: 0 };
      fitAndResize(state, true);

      // Should NOT have sent a resize with stale dimensions
      expect(state.resizeCalls).toHaveLength(1); // only the original call

      // Step 4: CSS layout completes, container gets correct split width
      state.containerEl = { clientWidth: 400, clientHeight: 600 };
      state.term = { rows: 24, cols: 40, focus: vi.fn() }; // narrower = fewer cols
      fitAndResize(state, true);

      expect(state.resizeCalls).toHaveLength(2);
      expect(state.resizeCalls[1]).toEqual({ rows: 24, cols: 40 });
    });
  });
});
