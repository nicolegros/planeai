import { describe, it, expect } from "vitest";
import {
  getComparison,
  setComparison,
  resetComparison,
  hasCustomComparison,
  formatComparison,
} from "../diff-comparison.svelte";

describe("diff comparison state", () => {
  it("returns default when no custom comparison is set", () => {
    const result = getComparison("session-1", "main");
    expect(result).toEqual({ baseRef: "main", headRef: null });
  });

  it("uses the provided defaultBase for unset sessions", () => {
    const result = getComparison("session-new", "develop");
    expect(result).toEqual({ baseRef: "develop", headRef: null });
  });

  it("setComparison stores a custom comparison", () => {
    setComparison("session-2", { baseRef: "develop", headRef: "abc123" });
    const result = getComparison("session-2", "main");
    expect(result).toEqual({ baseRef: "develop", headRef: "abc123" });
  });

  it("hasCustomComparison returns false for unset sessions", () => {
    expect(hasCustomComparison("session-never-set")).toBe(false);
  });

  it("hasCustomComparison returns true after setting", () => {
    setComparison("session-3", { baseRef: "main", headRef: "HEAD~1" });
    expect(hasCustomComparison("session-3")).toBe(true);
  });

  it("resetComparison removes custom comparison", () => {
    setComparison("session-4", { baseRef: "feat", headRef: "HEAD" });
    resetComparison("session-4");
    expect(hasCustomComparison("session-4")).toBe(false);
    const result = getComparison("session-4", "main");
    expect(result).toEqual({ baseRef: "main", headRef: null });
  });

  it("per-session isolation — different sessions have independent state", () => {
    setComparison("session-A", { baseRef: "branch-a", headRef: "sha-a" });
    setComparison("session-B", { baseRef: "branch-b", headRef: null });
    expect(getComparison("session-A", "main")).toEqual({ baseRef: "branch-a", headRef: "sha-a" });
    expect(getComparison("session-B", "main")).toEqual({ baseRef: "branch-b", headRef: null });
  });

  it("formatComparison shows base..head when headRef is set", () => {
    expect(formatComparison({ baseRef: "main", headRef: "abc123" })).toBe("main..abc123");
  });

  it("formatComparison shows Working tree when headRef is null", () => {
    expect(formatComparison({ baseRef: "main", headRef: null })).toBe("main..Working tree");
  });
});
