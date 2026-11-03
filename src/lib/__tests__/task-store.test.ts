import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskItem } from "../types";

const { listAll, move } = vi.hoisted(() => ({ listAll: vi.fn(), move: vi.fn() }));

const { listeners } = vi.hoisted(() => ({ listeners: new Map<string, () => void>() }));

vi.mock("../api", () => ({ tasks: { listAll, move } }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, handler: () => void) => {
    listeners.set(event, handler);
    return Promise.resolve(() => listeners.delete(event));
  }),
}));

import * as taskStore from "../task-store.svelte";

const task = (key: string, parentKey: string | null = null): TaskItem => ({
  key,
  title: key,
  status: "open",
  description: "",
  priority: 0,
  blocked_by: [],
  tags: [],
  parent_key: parentKey,
  url: null,
  base_branch: "main",
});

describe("task store", () => {
  beforeEach(async () => {
    listAll.mockReset();
    move.mockReset();
    await taskStore.refresh([]);
  });

  it("keeps the newest project paths when an older refresh finishes later", async () => {
    let completeOldRefresh!: (tasks: TaskItem[]) => void;
    listAll.mockImplementation((path: string) =>
      path === "/old"
        ? new Promise<TaskItem[]>((resolve) => {
            completeOldRefresh = resolve;
          })
        : Promise.resolve([task("NEW-1")]),
    );

    const oldRefresh = taskStore.refresh(["/old"]);
    await taskStore.refresh(["/new"]);
    completeOldRefresh([task("OLD-1")]);
    await oldRefresh;

    expect(taskStore.getTasksByProject()).toEqual({ "/new": [task("NEW-1")] });
  });

  it("retains successful snapshots when full or partial project reloads fail", async () => {
    const syncedTask = task("JIRA-1", "ABC-1");
    listAll.mockResolvedValueOnce([syncedTask]);
    await taskStore.loadTasks(["/repo"]);
    expect(taskStore.getTasksForProject("/repo")).toEqual([syncedTask]);

    listAll.mockRejectedValueOnce(new Error("database unavailable"));
    await taskStore.loadTasks(["/repo"]);
    expect(taskStore.getTasksForProject("/repo")).toEqual([syncedTask]);

    listAll.mockRejectedValueOnce(new Error("database unavailable"));
    await taskStore.refresh(["/repo"]);
    expect(taskStore.getTasksForProject("/repo")).toEqual([syncedTask]);
  });

  it("ignores an older response that completes after a newer refresh", async () => {
    let resolveStale!: (tasks: TaskItem[]) => void;
    listAll.mockImplementationOnce(
      () =>
        new Promise<TaskItem[]>((resolve) => {
          resolveStale = resolve;
        }),
    );
    const staleLoad = taskStore.loadTasks(["/repo"]);

    listAll.mockResolvedValueOnce([task("FRESH-1", "ABC-1")]);
    await taskStore.refresh(["/repo"]);
    resolveStale([task("STALE-1", "ABC-1")]);
    await staleLoad;

    expect(taskStore.getTasksForProject("/repo")).toEqual([task("FRESH-1", "ABC-1")]);
  });

  it("refreshes once after a burst of tasks-changed events", async () => {
    vi.useFakeTimers();
    listAll.mockResolvedValue([task("PLA-1")]);
    const stop = taskStore.startTaskEventListener(() => ["/repo"]);

    listeners.get("tasks-changed")!();
    listeners.get("tasks-changed")!();
    await vi.advanceTimersByTimeAsync(200);

    expect(listAll).toHaveBeenCalledTimes(1);
    expect(listAll).toHaveBeenCalledWith("/repo");
    stop();
    vi.useRealTimers();
  });

  describe("statuses ahead of the backend", () => {
    let statuses: Record<string, string>;
    const statusOf = (key: string) => taskStore.findTask("/repo", key)?.status;

    beforeEach(async () => {
      statuses = { "PLA-1": "in_progress" };
      listAll.mockImplementation(() =>
        Promise.resolve(
          Object.entries(statuses).map(([key, status]) => ({ ...task(key), status })),
        ),
      );
      await taskStore.loadTasks(["/repo"]);
    });

    it("keeps a held status across reloads until it is released", async () => {
      const hold = taskStore.holdStatus("PLA-1", "/repo", "done");
      expect(statusOf("PLA-1")).toBe("done");

      await taskStore.refresh(["/repo"]);
      await taskStore.loadTasks(["/repo"]);
      expect(statusOf("PLA-1")).toBe("done");

      taskStore.releaseStatus("PLA-1", "/repo", hold, "in_progress");
      expect(statusOf("PLA-1")).toBe("in_progress");
      statuses["PLA-1"] = "in_review";
      await taskStore.refresh(["/repo"]);
      expect(statusOf("PLA-1")).toBe("in_review");
    });

    it("keeps a hold that starts while a reload is in flight", async () => {
      let finishReload!: (tasks: TaskItem[]) => void;
      listAll.mockReturnValueOnce(new Promise<TaskItem[]>((resolve) => (finishReload = resolve)));

      const reloading = taskStore.refresh(["/repo"]);
      const hold = taskStore.holdStatus("PLA-1", "/repo", "done");
      finishReload([{ ...task("PLA-1"), status: "in_progress" }]);
      await reloading;

      expect(statusOf("PLA-1")).toBe("done");
      taskStore.releaseStatus("PLA-1", "/repo", hold, "in_progress");
    });

    it("leaves a newer hold in place when an older one is released", async () => {
      const older = taskStore.holdStatus("PLA-1", "/repo", "done");
      const newer = taskStore.holdStatus("PLA-1", "/repo", "in_review");
      taskStore.releaseStatus("PLA-1", "/repo", older, "in_progress");
      await taskStore.refresh(["/repo"]);

      expect(statusOf("PLA-1")).toBe("in_review");
      taskStore.releaseStatus("PLA-1", "/repo", newer);
    });

    it("shows a move before the reload that follows it lands", async () => {
      let finishReload!: (tasks: TaskItem[]) => void;
      move.mockResolvedValue(undefined);
      listAll.mockReturnValueOnce(new Promise<TaskItem[]>((resolve) => (finishReload = resolve)));

      const moving = taskStore.moveTask("PLA-1", "in_review", "/repo");
      expect(statusOf("PLA-1")).toBe("in_review");

      finishReload([{ ...task("PLA-1"), status: "in_review" }]);
      await moving;
      expect(statusOf("PLA-1")).toBe("in_review");
    });
  });
});
