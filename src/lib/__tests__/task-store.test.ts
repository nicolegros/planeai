import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskItem } from "../types";

const { listAll } = vi.hoisted(() => ({ listAll: vi.fn() }));

vi.mock("../api", () => ({ tasks: { listAll } }));

import { getTasksByProject, refresh } from "../task-store.svelte";

const task = (key: string): TaskItem => ({
  key,
  title: key,
  status: "open",
  description: "",
  priority: 0,
  blocked_by: [],
  tags: [],
  parent_key: null,
  url: null,
  base_branch: "main",
});

describe("task store refresh", () => {
  beforeEach(async () => {
    listAll.mockReset();
    await refresh([]);
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

    const oldRefresh = refresh(["/old"]);
    await refresh(["/new"]);
    completeOldRefresh([task("OLD-1")]);
    await oldRefresh;

    expect(getTasksByProject()).toEqual({ "/new": [task("NEW-1")] });
  });
});
