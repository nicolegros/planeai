import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../settings.svelte", () => ({
  getSettings: vi.fn(() => ({ post_merge_action: "archive" })),
}));
vi.mock("../snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import { getSettings } from "../settings.svelte";

import {
  showMergePrompt,
  getPrompt,
  getCountdown,
  handleKeep,
  handleArchive,
} from "../post-merge-prompt.svelte";

describe("post-merge-prompt countdown", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    handleKeep(); // reset state
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts at 30 when prompt is shown", () => {
    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive: vi.fn(() => Promise.resolve()),
      onDestroy: vi.fn(() => Promise.resolve()),
    });

    expect(getCountdown()).toBe(30);
    expect(getPrompt()).not.toBeNull();
  });

  it("decrements every second", () => {
    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive: vi.fn(() => Promise.resolve()),
      onDestroy: vi.fn(() => Promise.resolve()),
    });

    vi.advanceTimersByTime(1_000);
    expect(getCountdown()).toBe(29);

    vi.advanceTimersByTime(5_000);
    expect(getCountdown()).toBe(24);
  });

  it("reaches 0 after 30 seconds", () => {
    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive: vi.fn(() => Promise.resolve()),
      onDestroy: vi.fn(() => Promise.resolve()),
    });

    vi.advanceTimersByTime(30_000);
    expect(getCountdown()).toBe(0);
  });

  it("stops counting when user dismisses with handleKeep", () => {
    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive: vi.fn(() => Promise.resolve()),
      onDestroy: vi.fn(() => Promise.resolve()),
    });

    vi.advanceTimersByTime(5_000);
    expect(getCountdown()).toBe(25);

    handleKeep();
    // Countdown resets when prompt is dismissed
    expect(getCountdown()).toBe(0);
    vi.advanceTimersByTime(5_000);
    expect(getCountdown()).toBe(0);
  });

  it("stops counting when user acts with handleArchive", async () => {
    const onArchive = vi.fn(() => Promise.resolve());
    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive,
      onDestroy: vi.fn(() => Promise.resolve()),
    });

    vi.advanceTimersByTime(3_000);
    expect(getCountdown()).toBe(27);

    await handleArchive();
    // Countdown resets when prompt is acted on
    expect(getCountdown()).toBe(0);
    vi.advanceTimersByTime(5_000);
    expect(getCountdown()).toBe(0);
    expect(onArchive).toHaveBeenCalledWith("s1");
  });

  it("does not start countdown when post_merge_action is keep", () => {
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "keep" } as any);

    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive: vi.fn(() => Promise.resolve()),
      onDestroy: vi.fn(() => Promise.resolve()),
    });

    expect(getCountdown()).toBe(0);
    vi.advanceTimersByTime(5_000);
    expect(getCountdown()).toBe(0);

    // Restore
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "archive" } as any);
  });
});
