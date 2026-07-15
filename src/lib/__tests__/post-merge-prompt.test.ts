import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../settings.svelte", () => ({
  getSettings: vi.fn(() => ({ post_merge_action: "archive" })),
}));
vi.mock("../snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import { getSettings } from "../settings.svelte";
import { showSnackbar } from "../snackbar.svelte";

import {
  showMergePrompt,
  getPrompt,
  getCountdown,
  handleKeep,
  handleArchive,
  handleTaskDone,
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

describe("runDefault does not crash when session is already archived (PLA-248)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    handleKeep();
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "archive" } as any);
    vi.mocked(showSnackbar).mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not call onArchive when onTaskDone already archived the session", async () => {
    const onArchive = vi.fn(() => Promise.resolve());
    const onDestroy = vi.fn(() => Promise.resolve());
    // onTaskDone moves task to done — backend archives session behind the scenes
    const onTaskDone = vi.fn(() => Promise.resolve());

    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: "TASK-1",
      onArchive,
      onDestroy,
      onTaskDone,
    });

    // Let the 30s timeout fire (triggers runDefault)
    vi.advanceTimersByTime(30_000);

    // Let promises resolve
    await vi.waitFor(() => expect(onTaskDone).toHaveBeenCalledWith("s1"));
    // Allow the .then chain to resolve
    await Promise.resolve();
    await Promise.resolve();

    // onArchive should NOT be called — the session was already archived by onTaskDone
    // (backend archives sessions when task moves to done)
    expect(onArchive).not.toHaveBeenCalled();
    expect(showSnackbar).toHaveBeenCalledWith("Task done, session archived", "success");
  });

  it("does not call onDestroy when post_merge_action is destroy and task present", async () => {
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "destroy" } as any);
    const onArchive = vi.fn(() => Promise.resolve());
    const onDestroy = vi.fn(() => Promise.resolve());
    const onTaskDone = vi.fn(() => Promise.resolve());

    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: "TASK-1",
      onArchive,
      onDestroy,
      onTaskDone,
    });

    vi.advanceTimersByTime(30_000);
    await vi.waitFor(() => expect(onTaskDone).toHaveBeenCalledWith("s1"));
    await Promise.resolve();
    await Promise.resolve();

    expect(onDestroy).not.toHaveBeenCalled();
    expect(onArchive).not.toHaveBeenCalled();
    expect(showSnackbar).toHaveBeenCalledWith("Task done, session destroyed", "success");
  });

  it("still calls onArchive when session has no task", async () => {
    const onArchive = vi.fn(() => Promise.resolve());
    const onDestroy = vi.fn(() => Promise.resolve());

    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: null,
      onArchive,
      onDestroy,
    });

    vi.advanceTimersByTime(30_000);
    await Promise.resolve();
    await Promise.resolve();

    expect(onArchive).toHaveBeenCalledWith("s1");
    expect(showSnackbar).toHaveBeenCalledWith("Session auto-archived", "success");
  });

  it("handleTaskDone does not call onArchive", async () => {
    const onTaskDone = vi.fn(() => Promise.resolve());
    const onArchive = vi.fn(() => Promise.resolve());

    showMergePrompt({
      sessionId: "s1",
      sessionName: "test",
      taskKey: "TASK-1",
      onArchive,
      onDestroy: vi.fn(() => Promise.resolve()),
      onTaskDone,
    });

    // User presses "D" (Done)
    await handleTaskDone();
    expect(onTaskDone).toHaveBeenCalledWith("s1");
    expect(onArchive).not.toHaveBeenCalled();
  });
});
