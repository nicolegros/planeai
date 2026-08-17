import { beforeEach, describe, expect, it, vi } from "vitest";

const { getCombinedPatch } = vi.hoisted(() => ({
  getCombinedPatch: vi.fn(),
}));

vi.mock("../api", () => ({
  git: { getCombinedPatch },
}));

import { getRefreshedSelectedIndex } from "../diff-selection";
import {
  clearPreloadedPatches,
  disposePreloadedPatches,
  getCombinedPatchForReview,
  preloadPatches,
} from "../diff-preload";

describe("getCombinedPatchForReview", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearPreloadedPatches("session-1");
  });

  it("bypasses an Idle-time preload when manually refreshing", async () => {
    getCombinedPatch
      .mockResolvedValueOnce("stale patch from agent becoming idle")
      .mockResolvedValueOnce("fresh patch after requested changes");

    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(getCombinedPatch).toHaveBeenCalledTimes(1));

    await expect(
      getCombinedPatchForReview("session-1", "/repo", "main", null, false),
    ).resolves.toBe("fresh patch after requested changes");
    expect(getCombinedPatch).toHaveBeenCalledTimes(2);
  });

  it("uses a preload only for the initial review-tab load", async () => {
    getCombinedPatch.mockResolvedValueOnce("preloaded patch");
    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(getCombinedPatch).toHaveBeenCalledTimes(1));

    await expect(getCombinedPatchForReview("session-1", "/repo", "main", null, true)).resolves.toBe(
      "preloaded patch",
    );
    expect(getCombinedPatch).toHaveBeenCalledTimes(1);
  });

  it("does not use a default-comparison preload for a custom comparison", async () => {
    getCombinedPatch
      .mockResolvedValueOnce("default working-tree preload")
      .mockResolvedValueOnce("custom comparison patch");

    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(getCombinedPatch).toHaveBeenCalledTimes(1));

    await expect(
      getCombinedPatchForReview("session-1", "/repo", "release", "abc123", true),
    ).resolves.toBe("custom comparison patch");
    expect(getCombinedPatch).toHaveBeenCalledTimes(2);
  });

  it("keeps the most recently started preload when responses resolve out of order", async () => {
    let resolveFirst!: (patch: string) => void;
    let resolveSecond!: (patch: string) => void;
    getCombinedPatch
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            resolveSecond = resolve;
          }),
      );

    preloadPatches("session-1", "/repo", "main");
    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => {
      expect(getCombinedPatch).toHaveBeenCalledTimes(2);
      expect(resolveSecond).toBeTypeOf("function");
    });
    resolveSecond("newer preload");
    resolveFirst("older preload");
    await Promise.resolve();

    await expect(getCombinedPatchForReview("session-1", "/repo", "main", null, true)).resolves.toBe(
      "newer preload",
    );
  });

  it("does not serve an older resolved preload while a replacement is in flight", async () => {
    let resolveReplacement!: (patch: string) => void;
    getCombinedPatch
      .mockResolvedValueOnce("old resolved preload")
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            resolveReplacement = resolve;
          }),
      )
      .mockResolvedValueOnce("fresh patch because replacement is pending");

    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(getCombinedPatch).toHaveBeenCalledTimes(1));
    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(resolveReplacement).toBeTypeOf("function"));

    await expect(getCombinedPatchForReview("session-1", "/repo", "main", null, true)).resolves.toBe(
      "fresh patch because replacement is pending",
    );
    resolveReplacement("replacement preload");
  });
  it("fetches fresh data after terminal input invalidates a resolved preload", async () => {
    getCombinedPatch
      .mockResolvedValueOnce("preload before new terminal input")
      .mockResolvedValueOnce("fresh patch after new terminal input");

    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(getCombinedPatch).toHaveBeenCalledTimes(1));
    clearPreloadedPatches("session-1");

    await expect(getCombinedPatchForReview("session-1", "/repo", "main", null, true)).resolves.toBe(
      "fresh patch after new terminal input",
    );
    expect(getCombinedPatch).toHaveBeenCalledTimes(2);
  });

  it("does not allow a disposed session's late preload to populate a reused session ID", async () => {
    let resolvePreload!: (patch: string) => void;
    getCombinedPatch
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            resolvePreload = resolve;
          }),
      )
      .mockResolvedValueOnce("fresh preload for reused session ID");

    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(resolvePreload).toBeTypeOf("function"));
    disposePreloadedPatches("session-1");

    preloadPatches("session-1", "/repo", "main");
    await vi.waitFor(() => expect(getCombinedPatch).toHaveBeenCalledTimes(2));
    resolvePreload("late preload from disposed session");
    await Promise.resolve();

    await expect(getCombinedPatchForReview("session-1", "/repo", "main", null, true)).resolves.toBe(
      "fresh preload for reused session ID",
    );
  });
});

describe("diff refresh selection", () => {
  it("preserves the file selected while an in-flight refresh awaits its patch", async () => {
    let resolvePatch!: () => void;
    const patchLoaded = new Promise<void>((resolve) => {
      resolvePatch = resolve;
    });
    let files = [{ path: "first.ts" }, { path: "second.ts" }];
    let selectedIndex = 0;

    const pendingRefresh = patchLoaded.then(() => {
      selectedIndex = getRefreshedSelectedIndex(files, selectedIndex, [
        { path: "first.ts" },
        { path: "second.ts" },
      ]);
    });
    selectedIndex = 1;
    resolvePatch();
    await pendingRefresh;

    expect(selectedIndex).toBe(1);
  });
});
