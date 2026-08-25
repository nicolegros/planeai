import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskItem } from "../types";

const { listAll } = vi.hoisted(() => ({ listAll: vi.fn() }));

vi.mock("../api", () => ({ tasks: { listAll } }));

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
      () => new Promise<TaskItem[]>((resolve) => {
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
});
