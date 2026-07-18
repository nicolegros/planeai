import { describe, it, expect, beforeEach } from "vitest";
import {
  ensureSession,
  destroySession,
  getViewedFiles,
  setFileViewed,
  setFileUnviewed,
  isFileViewed,
  invalidateViewedFiles,
  clearViewedFiles,
  getViewedVersion,
} from "../diff-viewed.svelte";

describe("diff-viewed store", () => {
  beforeEach(() => {
    // Clear state between tests
    destroySession("test-session");
    destroySession("session-A");
    destroySession("session-B");
    destroySession("fresh-session");
    destroySession("brand-new");
  });

  it("returns empty set for a new session without ensureSession", () => {
    const files = getViewedFiles("fresh-session");
    expect(files.size).toBe(0);
  });

  it("ensureSession initializes state without triggering side effects on read", () => {
    ensureSession("test-session");
    const files = getViewedFiles("test-session");
    expect(files.size).toBe(0);
    expect(getViewedVersion("test-session")).toBe(0);
  });

  it("setFileViewed marks a file as viewed", () => {
    setFileViewed("test-session", "src/main.ts");
    expect(isFileViewed("test-session", "src/main.ts")).toBe(true);
    expect(getViewedFiles("test-session").has("src/main.ts")).toBe(true);
  });

  it("setFileUnviewed removes the mark", () => {
    setFileViewed("test-session", "src/main.ts");
    setFileUnviewed("test-session", "src/main.ts");
    expect(isFileViewed("test-session", "src/main.ts")).toBe(false);
  });

  it("isFileViewed returns false for unviewed files", () => {
    expect(isFileViewed("test-session", "nonexistent.ts")).toBe(false);
  });

  it("sessions are isolated from each other", () => {
    setFileViewed("session-A", "file-a.ts");
    setFileViewed("session-B", "file-b.ts");
    expect(isFileViewed("session-A", "file-a.ts")).toBe(true);
    expect(isFileViewed("session-A", "file-b.ts")).toBe(false);
    expect(isFileViewed("session-B", "file-b.ts")).toBe(true);
    expect(isFileViewed("session-B", "file-a.ts")).toBe(false);
  });

  it("clearViewedFiles resets all marks for a session", () => {
    setFileViewed("test-session", "a.ts");
    setFileViewed("test-session", "b.ts");
    clearViewedFiles("test-session");
    expect(getViewedFiles("test-session").size).toBe(0);
    expect(isFileViewed("test-session", "a.ts")).toBe(false);
  });

  it("destroySession removes all state for a session", () => {
    setFileViewed("test-session", "a.ts");
    destroySession("test-session");
    expect(isFileViewed("test-session", "a.ts")).toBe(false);
    expect(getViewedVersion("test-session")).toBe(0);
  });

  it("getViewedVersion starts at 0 for a fresh session", () => {
    expect(getViewedVersion("brand-new")).toBe(0);
  });

  it("version increments on setFileViewed", () => {
    ensureSession("test-session");
    const v0 = getViewedVersion("test-session");
    setFileViewed("test-session", "x.ts");
    expect(getViewedVersion("test-session")).toBe(v0 + 1);
  });

  it("version increments on setFileUnviewed", () => {
    setFileViewed("test-session", "x.ts");
    const v1 = getViewedVersion("test-session");
    setFileUnviewed("test-session", "x.ts");
    expect(getViewedVersion("test-session")).toBe(v1 + 1);
  });

  it("version increments on clearViewedFiles", () => {
    setFileViewed("test-session", "x.ts");
    const v1 = getViewedVersion("test-session");
    clearViewedFiles("test-session");
    expect(getViewedVersion("test-session")).toBe(v1 + 1);
  });

  describe("invalidateViewedFiles", () => {
    it("preserves viewed marks when fingerprints are unchanged", () => {
      const fingerprints = new Map([
        ["a.ts", "10:2:12"],
        ["b.ts", "5:1:6"],
      ]);
      // First call stores fingerprints
      invalidateViewedFiles("test-session", fingerprints);
      setFileViewed("test-session", "a.ts");
      setFileViewed("test-session", "b.ts");

      // Second call with same fingerprints — marks preserved
      invalidateViewedFiles("test-session", fingerprints);
      expect(isFileViewed("test-session", "a.ts")).toBe(true);
      expect(isFileViewed("test-session", "b.ts")).toBe(true);
    });

    it("invalidates viewed marks when fingerprint changes", () => {
      const fp1 = new Map([
        ["a.ts", "10:2:12"],
        ["b.ts", "5:1:6"],
      ]);
      invalidateViewedFiles("test-session", fp1);
      setFileViewed("test-session", "a.ts");
      setFileViewed("test-session", "b.ts");

      // a.ts fingerprint changed, b.ts stayed the same
      const fp2 = new Map([
        ["a.ts", "15:3:18"],
        ["b.ts", "5:1:6"],
      ]);
      invalidateViewedFiles("test-session", fp2);
      expect(isFileViewed("test-session", "a.ts")).toBe(false);
      expect(isFileViewed("test-session", "b.ts")).toBe(true);
    });

    it("does not invalidate files that were never viewed", () => {
      const fp1 = new Map([["a.ts", "10:2:12"]]);
      invalidateViewedFiles("test-session", fp1);

      // a.ts not marked as viewed — changing fingerprint shouldn't crash
      const fp2 = new Map([["a.ts", "20:4:24"]]);
      invalidateViewedFiles("test-session", fp2);
      expect(isFileViewed("test-session", "a.ts")).toBe(false);
    });

    it("does not invalidate viewed files that have no previous fingerprint", () => {
      // Mark a.ts viewed without ever calling invalidate (no stored fingerprint)
      setFileViewed("test-session", "a.ts");

      // First invalidate — no previous fingerprint for a.ts, so it should stay viewed
      const fp1 = new Map([["a.ts", "10:2:12"]]);
      invalidateViewedFiles("test-session", fp1);
      expect(isFileViewed("test-session", "a.ts")).toBe(true);
    });

    it("increments version when marks are invalidated", () => {
      const fp1 = new Map([["a.ts", "10:2:12"]]);
      invalidateViewedFiles("test-session", fp1);
      setFileViewed("test-session", "a.ts");
      const v = getViewedVersion("test-session");

      const fp2 = new Map([["a.ts", "20:4:24"]]);
      invalidateViewedFiles("test-session", fp2);
      expect(getViewedVersion("test-session")).toBeGreaterThan(v);
    });

    it("does not increment version when no marks are invalidated", () => {
      const fp1 = new Map([["a.ts", "10:2:12"]]);
      invalidateViewedFiles("test-session", fp1);
      setFileViewed("test-session", "a.ts");
      const v = getViewedVersion("test-session");

      // Same fingerprint — no invalidation
      invalidateViewedFiles("test-session", fp1);
      expect(getViewedVersion("test-session")).toBe(v);
    });
  });
});
