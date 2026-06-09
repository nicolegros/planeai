import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { touchMru, removeMru, getMruList } from "../mru.svelte";
import { getCycleState, startCycle, advance, commit, cancel } from "../tab-switcher.svelte";

function clearMru() {
  for (const id of getMruList()) removeMru(id);
}

function seedMruWith(...ids: string[]) {
  clearMru();
  // Touch in reverse so first id is most recent
  for (let i = ids.length - 1; i >= 0; i--) {
    touchMru(ids[i]);
  }
}

describe("tab-switcher state machine", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Reset any lingering cycle
    cancel();
    clearMru();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("startCycle returns false with 0 other sessions", () => {
    seedMruWith("a");
    expect(startCycle("a")).toBe(false);
    expect(getCycleState().isCycling).toBe(false);
  });

  it("startCycle returns true and sets isCycling", () => {
    seedMruWith("a", "b", "c");
    expect(startCycle("a")).toBe(true);
    expect(getCycleState().isCycling).toBe(true);
    expect(getCycleState().isVisible).toBe(false);
  });

  it("overlay becomes visible after 150ms", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    vi.advanceTimersByTime(149);
    expect(getCycleState().isVisible).toBe(false);
    vi.advanceTimersByTime(1);
    expect(getCycleState().isVisible).toBe(true);
  });

  it("index starts at 0 (next MRU session)", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    expect(getCycleState().index).toBe(0);
    // cycleList should be [b, c, a] — others first, current last
    expect(getCycleState().cycleList).toEqual(["b", "c", "a"]);
  });

  it("advance moves index forward and wraps", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    advance(1);
    expect(getCycleState().index).toBe(1);
    advance(1);
    expect(getCycleState().index).toBe(2);
    advance(1);
    expect(getCycleState().index).toBe(0); // wraps
  });

  it("advance moves index backward and wraps", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    advance(-1);
    expect(getCycleState().index).toBe(2); // wraps to end
  });

  it("commit returns selected session and resets state", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    advance(1);
    const target = commit();
    expect(target).toBe("c");
    expect(getCycleState().isCycling).toBe(false);
    expect(getCycleState().isVisible).toBe(false);
  });

  it("cancel returns origin session and resets state", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    advance(1);
    const origin = cancel();
    expect(origin).toBe("a");
    expect(getCycleState().isCycling).toBe(false);
  });

  // --- Bug reproduction: selector disappears ---

  it("rapid Ctrl+Tab: commit before overlay visible should not corrupt next cycle", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    // User releases Ctrl before 150ms (fast switch)
    const target = commit();
    expect(target).toBe("b");
    // Timer should be cleared — no late isVisible flip
    vi.advanceTimersByTime(200);
    expect(getCycleState().isVisible).toBe(false);
    expect(getCycleState().isCycling).toBe(false);
  });

  it("MRU mutation during visible cycle does NOT affect cycleList", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    vi.advanceTimersByTime(150);
    expect(getCycleState().isVisible).toBe(true);

    // Simulate external MRU mutation (e.g., session-created event)
    touchMru("d");

    // cycleList should be unaffected
    expect(getCycleState().cycleList).toEqual(["b", "c", "a"]);
    expect(getCycleState().index).toBe(0);
    expect(getCycleState().isVisible).toBe(true);
  });

  it("selectedIndex always in bounds of cycleList during cycling", () => {
    seedMruWith("a", "b", "c", "d", "e");
    startCycle("a");
    vi.advanceTimersByTime(150);

    // Advance many times — index must always be in bounds
    for (let i = 0; i < 20; i++) {
      advance(1);
      const { index, cycleList } = getCycleState();
      expect(index).toBeGreaterThanOrEqual(0);
      expect(index).toBeLessThan(cycleList.length);
    }
  });

  it("double startCycle without commit does not corrupt state", () => {
    seedMruWith("a", "b", "c");
    startCycle("a");
    vi.advanceTimersByTime(150);
    expect(getCycleState().isVisible).toBe(true);

    // What happens if startCycle is called again while cycling?
    // This shouldn't happen via the UI (guarded by !switcher.isCycling)
    // but let's verify the state machine handles it
    startCycle("b");
    vi.advanceTimersByTime(150);
    expect(getCycleState().isVisible).toBe(true);
    expect(getCycleState().isCycling).toBe(true);
  });

  it("commit during visible cycle with index > 0 returns correct target", () => {
    seedMruWith("a", "b", "c", "d");
    startCycle("a");
    vi.advanceTimersByTime(150);
    advance(1);
    advance(1);
    // index is 2, cycleList is [b, c, d, a]
    expect(getCycleState().index).toBe(2);
    const target = commit();
    expect(target).toBe("d");
    expect(getCycleState().isVisible).toBe(false);
  });

  it("startCycle with undefined activeSessionId uses full MRU as cycleList", () => {
    seedMruWith("a", "b", "c");
    startCycle(undefined);
    // No current session to append, cycleList = full MRU
    expect(getCycleState().cycleList).toEqual(["a", "b", "c"]);
  });

  it("startCycle with activeSessionId NOT in MRU still works", () => {
    seedMruWith("a", "b", "c");
    const result = startCycle("z");
    expect(result).toBe(true);
    // "z" not in MRU, so others = full MRU, current = null
    expect(getCycleState().cycleList).toEqual(["a", "b", "c"]);
  });

  it("startCycle with validIds filters out stale MRU entries", () => {
    seedMruWith("a", "b", "ghost", "c");
    const validIds = new Set(["a", "b", "c"]);
    startCycle("a", validIds);
    // "ghost" should be excluded from cycleList
    expect(getCycleState().cycleList).toEqual(["b", "c", "a"]);
    expect(getCycleState().index).toBe(0);
    // Committing should return "b", not "ghost"
    expect(commit()).toBe("b");
  });

  it("startCycle returns false when validIds filters all others", () => {
    seedMruWith("a", "ghost1", "ghost2");
    const validIds = new Set(["a"]);
    expect(startCycle("a", validIds)).toBe(false);
    expect(getCycleState().isCycling).toBe(false);
  });
});
