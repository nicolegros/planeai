import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../settings.svelte", () => ({
  getSettings: vi.fn(() => ({ post_merge_action: "archive" })),
}));

import { getSettings } from "../settings.svelte";
import {
  _resetForTests,
  getToasts,
  requestDismissToast,
  runToastAction,
  showToast,
} from "../toasts.svelte";
import {
  dismissForSession,
  showMergePrompt,
  type MergePromptOptions,
} from "../post-merge-prompt.svelte";

vi.mock("../toasts.svelte", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../toasts.svelte")>();
  return { ...actual, showToast: vi.fn(actual.showToast) };
});

function prompt(overrides: Partial<MergePromptOptions> = {}): MergePromptOptions {
  return {
    sessionId: "s1",
    sessionName: "test",
    taskKey: null,
    onArchive: vi.fn(() => Promise.resolve()),
    onDestroy: vi.fn(() => Promise.resolve()),
    ...overrides,
  };
}

function only() {
  const toasts = getToasts();
  expect(toasts).toHaveLength(1);
  return toasts[0];
}

/** Let handler promises and their .then chains settle. */
async function settle() {
  for (let i = 0; i < 4; i += 1) await Promise.resolve();
}

beforeEach(() => {
  vi.useFakeTimers();
  _resetForTests();
  vi.mocked(getSettings).mockReturnValue({ post_merge_action: "archive" } as any);
  vi.mocked(showToast).mockClear();
});

afterEach(() => {
  _resetForTests();
  vi.useRealTimers();
});

describe("post-merge prompt toast", () => {
  it("offers archive and destroy, with dismiss as keep, for a session without a task", () => {
    showMergePrompt(prompt());

    const toast = only();
    expect(toast.kind).toBe("success");
    expect(toast.message).toBe("PR merged ✓");
    expect(toast.detail).toBe("test");
    expect(toast.actions.map(({ key, label }) => [key, label])).toEqual([
      ["a", "Archive"],
      ["x", "Destroy"],
    ]);
    expect(toast.dismissLabel).toBe("Keep");
  });

  it("offers done on Enter for a session with a task", () => {
    showMergePrompt(prompt({ taskKey: "TASK-1", onTaskDone: vi.fn(() => Promise.resolve()) }));

    expect(only().actions.map(({ key, label }) => [key, label])).toEqual([["Enter", "Done"]]);
  });

  it("runs the chosen action for the session", async () => {
    const options = prompt();
    showMergePrompt(options);
    runToastAction(only().id, "x");
    expect(options.onDestroy).toHaveBeenCalledWith("s1");
    expect(getToasts()).toHaveLength(0);

    showMergePrompt(options);
    runToastAction(only().id, "a");
    expect(options.onArchive).toHaveBeenCalledWith("s1");
  });

  it("keeps the session when dismissed and stops the countdown", async () => {
    const options = prompt();
    showMergePrompt(options);

    requestDismissToast(only().id);
    vi.advanceTimersByTime(60_000);
    await settle();

    expect(getToasts()).toHaveLength(0);
    expect(options.onArchive).not.toHaveBeenCalled();
    expect(options.onDestroy).not.toHaveBeenCalled();
  });

  it("counts down from 30s with the default action in the label", () => {
    showMergePrompt(prompt());
    expect(only().countdownLabel).toBe("auto-archives in 30s");

    vi.advanceTimersByTime(1_000);
    expect(only().countdownLabel).toBe("auto-archives in 29s");
    vi.advanceTimersByTime(5_000);
    expect(only().countdownLabel).toBe("auto-archives in 24s");

    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "destroy" } as any);
    vi.advanceTimersByTime(1_000);
    expect(only().countdownLabel).toBe("auto-destroys in 23s");
  });

  it("does not count down when post_merge_action is keep", () => {
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "keep" } as any);
    const options = prompt();
    showMergePrompt(options);

    expect(only().countdownLabel).toBeUndefined();
    vi.advanceTimersByTime(60_000);
    expect(getToasts()).toHaveLength(1);
    expect(options.onArchive).not.toHaveBeenCalled();
  });

  it("carries a host-rendered integration message", () => {
    showMergePrompt(prompt({ message: "Deployment completed" }));
    expect(only().message).toBe("Deployment completed ✓");
  });

  it("shows one prompt per session and replaces a repeated one", () => {
    showMergePrompt(prompt());
    showMergePrompt(prompt({ sessionId: "s2", sessionName: "other" }));
    showMergePrompt(prompt({ sessionName: "renamed" }));

    expect(getToasts().map((toast) => toast.detail)).toEqual(["renamed", "other"]);
  });

  it("closes the prompt of a session removed elsewhere", () => {
    const options = prompt();
    showMergePrompt(options);
    showMergePrompt(prompt({ sessionId: "s2" }));

    dismissForSession("s1");
    vi.advanceTimersByTime(60_000);

    expect(options.onArchive).not.toHaveBeenCalled();
    expect(getToasts()).toHaveLength(0);
  });
});

describe("default action does not crash when session is already archived (PLA-248)", () => {
  it("does not call onArchive when onTaskDone already archived the session", async () => {
    const options = prompt({ taskKey: "TASK-1", onTaskDone: vi.fn(() => Promise.resolve()) });
    showMergePrompt(options);

    vi.advanceTimersByTime(30_000);
    await settle();

    // Moving the task to done archives the session in the backend.
    expect(options.onTaskDone).toHaveBeenCalledWith("s1");
    expect(options.onArchive).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith("Task done, session archived", "success");
  });

  it("does not call onDestroy when post_merge_action is destroy and task present", async () => {
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "destroy" } as any);
    const options = prompt({ taskKey: "TASK-1", onTaskDone: vi.fn(() => Promise.resolve()) });
    showMergePrompt(options);

    vi.advanceTimersByTime(30_000);
    await settle();

    expect(options.onTaskDone).toHaveBeenCalledWith("s1");
    expect(options.onDestroy).not.toHaveBeenCalled();
    expect(options.onArchive).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith("Task done, session destroyed", "success");
  });

  it("still calls onArchive when session has no task", async () => {
    const options = prompt();
    showMergePrompt(options);

    vi.advanceTimersByTime(30_000);
    await settle();

    expect(options.onArchive).toHaveBeenCalledWith("s1");
    expect(showToast).toHaveBeenCalledWith("Session auto-archived", "success");
  });

  it("reports a failed default action", async () => {
    vi.mocked(getSettings).mockReturnValue({ post_merge_action: "destroy" } as any);
    showMergePrompt(prompt({ onDestroy: vi.fn(() => Promise.reject(new Error("busy"))) }));

    vi.advanceTimersByTime(30_000);
    await settle();

    expect(showToast).toHaveBeenCalledWith("Error: busy", "error");
  });

  it("Done does not call onArchive", () => {
    const options = prompt({ taskKey: "TASK-1", onTaskDone: vi.fn(() => Promise.resolve()) });
    showMergePrompt(options);

    runToastAction(only().id, "Enter");
    expect(options.onTaskDone).toHaveBeenCalledWith("s1");
    expect(options.onArchive).not.toHaveBeenCalled();
  });
});
