import { describe, expect, it, vi } from "vitest";

const { listAll } = vi.hoisted(() => ({ listAll: vi.fn() }));

vi.mock("../api", () => ({
  tasks: { listAll },
}));

import * as taskStore from "../task-store.svelte";

const task = {
  key: "JIRA-1",
  title: "Jira task",
  status: "todo",
  description: "",
  priority: 0,
  blocked_by: [],
  tags: [],
  parent_key: "ABC-1",
  url: null,
  base_branch: "main",
};

describe("task-store", () => {
  it("retains successful snapshots when full or partial project reloads fail", async () => {
    listAll.mockResolvedValueOnce([task]);
    await taskStore.loadTasks(["/repo"]);
    expect(taskStore.getTasksForProject("/repo")).toEqual([task]);

    listAll.mockRejectedValueOnce(new Error("database unavailable"));
    await taskStore.loadTasks(["/repo"]);
    expect(taskStore.getTasksForProject("/repo")).toEqual([task]);

    listAll.mockRejectedValueOnce(new Error("database unavailable"));
    await taskStore.refresh(["/repo"]);
    expect(taskStore.getTasksForProject("/repo")).toEqual([task]);
  });

  it("ignores an older response that completes after a newer refresh", async () => {
    const staleTask = { ...task, key: "STALE-1" };
    const freshTask = { ...task, key: "FRESH-1" };
    let resolveStale: (tasks: (typeof task)[]) => void;
    listAll.mockImplementationOnce(
      () => new Promise<(typeof task)[]>((resolve) => (resolveStale = resolve)),
    );
    const staleLoad = taskStore.loadTasks(["/repo"]);

    listAll.mockResolvedValueOnce([freshTask]);
    await taskStore.refresh(["/repo"]);
    resolveStale!([staleTask]);
    await staleLoad;

    expect(taskStore.getTasksForProject("/repo")).toEqual([freshTask]);
  });
});
