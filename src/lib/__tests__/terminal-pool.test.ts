import { describe, it, expect, beforeEach } from "vitest";
import { getPoolState, activateSession, removeSession, MAX_MOUNTED } from "../terminal-pool.svelte";

describe("terminal-pool", () => {
  beforeEach(() => {
    // Reset pool state by removing all sessions
    const state = getPoolState();
    for (const id of state.mounted) {
      removeSession(id);
    }
  });

  it("mounts the active session", () => {
    activateSession("s1");
    const state = getPoolState();
    expect(state.mounted).toContain("s1");
    expect(state.active).toBe("s1");
  });

  it("keeps MRU neighbors mounted (up to MAX_MOUNTED)", () => {
    activateSession("s1");
    activateSession("s2");
    activateSession("s3");
    const state = getPoolState();
    // s3 is active, s2 is MRU-1, s1 is MRU-2
    expect(state.active).toBe("s3");
    expect(state.mounted).toContain("s3");
    expect(state.mounted).toContain("s2");
    expect(state.mounted.length).toBeLessThanOrEqual(MAX_MOUNTED);
  });

  it("unmounts sessions beyond MAX_MOUNTED", () => {
    // Activate more than MAX_MOUNTED sessions
    activateSession("s1");
    activateSession("s2");
    activateSession("s3");
    activateSession("s4");
    const state = getPoolState();
    expect(state.mounted.length).toBeLessThanOrEqual(MAX_MOUNTED);
    // s4 is active, s3 and s2 are neighbors
    expect(state.mounted).toContain("s4");
    expect(state.mounted).toContain("s3");
    // s1 should be evicted
    expect(state.mounted).not.toContain("s1");
  });

  it("switching back promotes a session without exceeding MAX_MOUNTED", () => {
    activateSession("s1");
    activateSession("s2");
    activateSession("s3");
    activateSession("s4");
    // Now switch back to s1
    activateSession("s1");
    const state = getPoolState();
    expect(state.active).toBe("s1");
    expect(state.mounted).toContain("s1");
    expect(state.mounted.length).toBeLessThanOrEqual(MAX_MOUNTED);
  });

  it("removeSession evicts from mounted set", () => {
    activateSession("s1");
    activateSession("s2");
    removeSession("s1");
    const state = getPoolState();
    expect(state.mounted).not.toContain("s1");
  });

  it("isMounted returns correct state for each session", () => {
    activateSession("s1");
    activateSession("s2");
    activateSession("s3");
    activateSession("s4");
    const state = getPoolState();
    expect(state.mounted).toContain("s4");
    expect(state.mounted).toContain("s3");
    expect(state.mounted).not.toContain("s1");
  });

  it("shouldPause returns true for mounted but non-active sessions", () => {
    activateSession("s1");
    activateSession("s2");
    activateSession("s3");
    const state = getPoolState();
    // s3 is active (not paused), s2 is mounted but paused
    expect(state.active).toBe("s3");
    expect(state.paused).toContain("s2");
    expect(state.paused).not.toContain("s3");
  });
});
