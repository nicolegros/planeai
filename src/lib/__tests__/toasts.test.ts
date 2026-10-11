import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  _resetForTests,
  dismissToast,
  getToasts,
  holdToast,
  releaseToast,
  requestDismissToast,
  runToastAction,
  showToast,
  TOAST_LIMIT,
} from "../toasts.svelte";

describe("toasts", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    _resetForTests();
  });

  afterEach(() => {
    _resetForTests();
    vi.useRealTimers();
  });

  it("shows plain toasts as errors by default, newest last", () => {
    showToast("first");
    showToast("second", "warning");

    expect(getToasts().map((toast) => [toast.message, toast.kind])).toEqual([
      ["first", "error"],
      ["second", "warning"],
    ]);
  });

  it("bumps the count of a repeated plain toast instead of adding a row", () => {
    const id = showToast("Failed to save settings");
    showToast("Failed to save settings", "warning");
    const again = showToast("Failed to save settings");

    expect(again).toBe(id);
    expect(getToasts()).toHaveLength(2);
    expect(getToasts().find((toast) => toast.id === id)?.count).toBe(2);
  });

  it("restarts the auto-dismiss timer of a repeated success toast", () => {
    showToast("Saved", "success");
    vi.advanceTimersByTime(2_000);
    showToast("Saved", "success");
    vi.advanceTimersByTime(2_000);
    expect(getToasts()).toHaveLength(1);
    vi.advanceTimersByTime(1_000);
    expect(getToasts()).toHaveLength(0);
  });

  it("never merges toasts that carry actions", () => {
    const action = { key: "i", label: "Install", run: vi.fn() };
    showToast({ message: "Update", kind: "info", actions: [action] });
    showToast({ message: "Update", kind: "info", actions: [action] });

    expect(getToasts()).toHaveLength(2);
  });

  it("auto-dismisses success toasts after 3s and keeps errors", () => {
    showToast("Done", "success");
    showToast("Broken");

    vi.advanceTimersByTime(2_999);
    expect(getToasts()).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(getToasts().map((toast) => toast.message)).toEqual(["Broken"]);
    vi.advanceTimersByTime(60_000);
    expect(getToasts()).toHaveLength(1);
  });

  it("honors a caller-provided auto-dismiss delay", () => {
    showToast({ message: "Heads up", kind: "info", autoDismissMs: 500 });
    vi.advanceTimersByTime(500);
    expect(getToasts()).toHaveLength(0);
  });

  it("evicts the oldest auto-dismissible toast first, then the oldest plain one", () => {
    const action = { key: "a", label: "Archive", run: vi.fn() };
    showToast({ message: "actionable", actions: [action] });
    showToast("sticky 1");
    showToast("transient", "success");
    showToast("sticky 2");
    showToast("sticky 3");

    expect(getToasts()).toHaveLength(TOAST_LIMIT);
    expect(getToasts().map((toast) => toast.message)).toEqual([
      "actionable",
      "sticky 1",
      "sticky 2",
      "sticky 3",
    ]);

    showToast("sticky 4");
    expect(getToasts().map((toast) => toast.message)).toEqual([
      "actionable",
      "sticky 2",
      "sticky 3",
      "sticky 4",
    ]);
  });

  it("never evicts actionable or counting-down toasts", () => {
    for (let index = 0; index < TOAST_LIMIT; index += 1) {
      showToast({
        message: `prompt ${index}`,
        countdown: { seconds: 30, label: (remaining) => `${remaining}s`, onExpire: vi.fn() },
      });
    }
    showToast("newest error");

    expect(getToasts()).toHaveLength(TOAST_LIMIT + 1);
    expect(getToasts().at(-1)?.message).toBe("newest error");
  });

  it("pauses the auto-dismiss timer while held and resumes with the remaining time", () => {
    const id = showToast("Done", "success");
    vi.advanceTimersByTime(2_000);
    holdToast(id, "focus");
    vi.advanceTimersByTime(10_000);
    expect(getToasts()).toHaveLength(1);

    releaseToast(id, "focus");
    vi.advanceTimersByTime(999);
    expect(getToasts()).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(getToasts()).toHaveLength(0);
  });

  it("stays paused until every hold is released", () => {
    const id = showToast("Done", "success");
    holdToast(id, "focus");
    holdToast(id, "hover");
    releaseToast(id, "focus");
    vi.advanceTimersByTime(10_000);
    expect(getToasts()).toHaveLength(1);
    releaseToast(id, "hover");
    vi.advanceTimersByTime(3_000);
    expect(getToasts()).toHaveLength(0);
  });

  it("ticks a countdown, pauses it while held and runs onExpire at zero", () => {
    const onExpire = vi.fn();
    const id = showToast({
      message: "PR merged",
      kind: "success",
      countdown: { seconds: 30, label: (remaining) => `auto-archives in ${remaining}s`, onExpire },
    });
    const countdownLabel = () => getToasts()[0]?.countdownLabel;

    expect(countdownLabel()).toBe("auto-archives in 30s");
    vi.advanceTimersByTime(1_500);
    expect(countdownLabel()).toBe("auto-archives in 29s");

    holdToast(id, "hover");
    vi.advanceTimersByTime(60_000);
    expect(countdownLabel()).toBe("auto-archives in 29s");
    expect(onExpire).not.toHaveBeenCalled();

    releaseToast(id, "hover");
    vi.advanceTimersByTime(28_499);
    expect(countdownLabel()).toBe("auto-archives in 1s");
    vi.advanceTimersByTime(1);
    expect(onExpire).toHaveBeenCalledOnce();
    expect(getToasts()).toHaveLength(0);
  });

  it("rejects actions bound to reserved or invalid keys", () => {
    for (const key of ["d", "c", "Escape", "ArrowUp", "ArrowDown", "D", "ab", "1"]) {
      expect(() =>
        showToast({ message: "x", actions: [{ key, label: "Nope", run: vi.fn() }] }),
      ).toThrow();
    }
    expect(() =>
      showToast({
        message: "x",
        actions: [
          { key: "a", label: "One", run: vi.fn() },
          { key: "a", label: "Two", run: vi.fn() },
        ],
      }),
    ).toThrow();
    expect(getToasts()).toHaveLength(0);
  });

  it("removes the toast and runs the action bound to a key", async () => {
    const run = vi.fn();
    const id = showToast({ message: "Update", actions: [{ key: "Enter", label: "Done", run }] });

    expect(runToastAction(id, "x")).toBe(false);
    expect(runToastAction(id, "Enter")).toBe(true);
    expect(run).toHaveBeenCalledOnce();
    expect(getToasts()).toHaveLength(0);
  });

  it("lets an action replace its own toast by id", () => {
    const id = showToast({
      id: "update",
      message: "Update available",
      actions: [
        {
          key: "i",
          label: "Install",
          run: () => void showToast({ id: "update", message: "Installing", dismiss: "disabled" }),
        },
      ],
    });

    runToastAction(id, "i");

    expect(getToasts().map((toast) => [toast.id, toast.message])).toEqual([
      ["update", "Installing"],
    ]);
  });

  it("surfaces a failing action as an error toast", async () => {
    const id = showToast({
      message: "Merge",
      actions: [{ key: "a", label: "Archive", run: () => Promise.reject(new Error("boom")) }],
    });

    runToastAction(id, "a");
    await vi.waitFor(() =>
      expect(getToasts().map((toast) => toast.message)).toEqual(["Error: boom"]),
    );
  });

  it("runs the custom dismiss handler and refuses a disabled dismiss", () => {
    const onDismiss = vi.fn();
    const custom = showToast({ message: "Update", dismiss: onDismiss });
    const locked = showToast({ message: "Installing", dismiss: "disabled" });

    expect(requestDismissToast(locked)).toBe(false);
    expect(requestDismissToast(custom)).toBe(true);
    expect(onDismiss).toHaveBeenCalledOnce();
    expect(getToasts().map((toast) => toast.id)).toEqual([locked]);
  });

  it("removes a toast programmatically without running its handlers", () => {
    const onDismiss = vi.fn();
    const onExpire = vi.fn();
    const id = showToast({
      message: "PR merged",
      dismiss: onDismiss,
      countdown: { seconds: 1, label: () => "", onExpire },
    });

    dismissToast(id);
    vi.advanceTimersByTime(5_000);

    expect(getToasts()).toHaveLength(0);
    expect(onDismiss).not.toHaveBeenCalled();
    expect(onExpire).not.toHaveBeenCalled();
  });
});
