import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskItem } from "../types";

const { listAll, move, markVisited, markUnread, snoozeApi, wakeApi } = vi.hoisted(() => ({
  listAll: vi.fn(),
  move: vi.fn(),
  markVisited: vi.fn(),
  markUnread: vi.fn(),
  snoozeApi: vi.fn(),
  wakeApi: vi.fn(),
}));

const { listeners } = vi.hoisted(() => ({
  listeners: new Map<string, (event?: { payload: unknown }) => void>(),
}));

vi.mock("../api", () => ({
  tasks: { listAll, move, markVisited, markUnread, snooze: snoozeApi, wake: wakeApi },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, handler: (event?: { payload: unknown }) => void) => {
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
  last_visited_at: null,
  last_activity_at: null,
  snoozed_until: null,
  snoozed_at: null,
  woke_at: null,
  snooze_rev: 0,
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

describe("task visits", () => {
  const visitOf = (key: string) => {
    const found = taskStore.findTask("/repo", key);
    return (
      found && { last_visited_at: found.last_visited_at, last_activity_at: found.last_activity_at }
    );
  };

  beforeEach(async () => {
    listAll.mockReset();
    markVisited.mockReset();
    markUnread.mockReset();
    snoozeApi.mockReset();
    wakeApi.mockReset();
    await taskStore.refresh([]);
    listAll.mockResolvedValue([task("PLA-1"), task("PLA-2")]);
    await taskStore.loadTasks(["/repo"]);
    listAll.mockClear();
  });

  it("shows activity on a task the user is not looking at, without reloading", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    listeners.get("task-activity")?.({
      payload: {
        project_id: "p1",
        task_key: "PLA-1",
        last_visited_at: null,
        last_activity_at: "2026-11-03T10:00:00.000Z",
        snoozed_until: null,
        snoozed_at: null,
        woke_at: null,
      },
    });
    await Promise.resolve();

    expect([visitOf("PLA-1"), visitOf("PLA-2")]).toEqual([
      { last_visited_at: null, last_activity_at: "2026-11-03T10:00:00.000Z" },
      { last_visited_at: null, last_activity_at: null },
    ]);
    expect(listAll).not.toHaveBeenCalled();
    stop();
  });

  const snoozed = {
    snoozed_until: "2026-11-04T09:00:00.000Z",
    snoozed_at: "2026-11-03T09:00:00.000Z",
    woke_at: null,
    snooze_rev: 1,
  };
  const woken = { ...snoozed, woke_at: "2026-11-03T10:00:00.000Z", snooze_rev: 2 };
  const resnoozed = {
    snoozed_until: "2026-11-05T09:00:00.000Z",
    snoozed_at: "2026-11-03T10:00:00.500Z",
    woke_at: null,
    snooze_rev: 3,
  };

  function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<T>((done, fail) => ((resolve = done), (reject = fail)));
    return { promise, resolve, reject };
  }

  function activity(woke: typeof woken | null): void {
    listeners.get("task-activity")?.({
      payload: {
        project_id: "p1",
        task_key: "PLA-1",
        last_visited_at: null,
        last_activity_at: "2026-11-03T10:00:01.000Z",
        woke,
      },
    });
  }

  const snoozeInStore = () => {
    const { snoozed_until, snoozed_at, woke_at, snooze_rev } = taskStore.findTask(
      "/repo",
      "PLA-1",
    )!;
    return { snoozed_until, snoozed_at, woke_at, snooze_rev };
  };

  const snoozeTomorrow = (reply: Promise<typeof snoozed>) => {
    snoozeApi.mockReturnValueOnce(reply);
    return taskStore.snoozeTask("p1", "PLA-1", new Date(snoozed.snoozed_until));
  };

  it("shows an early wake its event carries, without reloading", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    await snoozeTomorrow(Promise.resolve(snoozed));

    activity(woken);

    expect(snoozeInStore()).toEqual(woken);
    expect(listAll).not.toHaveBeenCalled();
    stop();
  });

  it("keeps an early wake whose event beats the reply to the snooze it woke", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    const reply = deferred<typeof snoozed>();
    const snoozing = snoozeTomorrow(reply.promise);

    activity(woken);
    reply.resolve(snoozed);

    await expect(snoozing).resolves.toEqual({
      snoozed_until: snoozed.snoozed_until,
      snoozed_at: snoozed.snoozed_at,
      woke_at: null,
    });
    expect(snoozeInStore()).toEqual(woken);
    stop();
  });

  it("keeps a snooze made after an early wake whose event comes late", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    await snoozeTomorrow(Promise.resolve(snoozed));
    snoozeApi.mockResolvedValueOnce(resnoozed);
    await taskStore.snoozeTask("p1", "PLA-1", new Date(resnoozed.snoozed_until));

    activity(woken);

    expect(snoozeInStore()).toEqual(resnoozed);
    stop();
  });

  it("keeps an early wake over a reload that read the database before it", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    await snoozeTomorrow(Promise.resolve(snoozed));
    const reload = deferred<TaskItem[]>();
    listAll.mockReturnValueOnce(reload.promise);

    const reloading = taskStore.refresh(["/repo"]);
    activity(woken);
    reload.resolve([{ ...task("PLA-1"), ...snoozed, title: "renamed" }, task("PLA-2")]);
    await reloading;

    expect(snoozeInStore()).toEqual(woken);
    expect(taskStore.findTask("/repo", "PLA-1")?.title).toBe("renamed");
    stop();
  });

  it("takes a newer snooze from a reload, such as a wake from the CLI", async () => {
    await snoozeTomorrow(Promise.resolve(snoozed));
    const wokenFromCli = { snoozed_until: null, snoozed_at: null, woke_at: null, snooze_rev: 2 };
    listAll.mockResolvedValueOnce([{ ...task("PLA-1"), ...wokenFromCli }, task("PLA-2")]);

    await taskStore.loadTasks(["/repo"]);

    expect(snoozeInStore()).toEqual(wokenFromCli);
  });

  it("stays on the newest snooze it saw when a write fails", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    await snoozeTomorrow(Promise.resolve(snoozed));
    const reply = deferred<typeof snoozed>();
    wakeApi.mockReturnValueOnce(reply.promise);

    const waking = taskStore.wakeTask("p1", "PLA-1");
    activity(woken);
    reply.reject(new Error("database is locked"));

    await expect(waking).rejects.toThrow("database is locked");
    expect(snoozeInStore()).toEqual(woken);
    stop();
  });

  it("leaves the snooze alone on activity that woke nothing", async () => {
    const stop = taskStore.startTaskActivityListener(() => false);
    await snoozeTomorrow(Promise.resolve(snoozed));

    activity(null);

    expect(snoozeInStore()).toEqual(snoozed);
    stop();
  });

  it("marks the task visited when the user is looking at it as its agent settles", async () => {
    markVisited.mockResolvedValue({
      last_visited_at: "2026-11-03T10:00:00.010Z",
      last_activity_at: "2026-11-03T10:00:00.000Z",
    });
    const stop = taskStore.startTaskActivityListener(
      (projectId, key) => projectId === "p1" && key === "PLA-1",
    );
    listeners.get("task-activity")?.({
      payload: {
        project_id: "p1",
        task_key: "PLA-1",
        last_visited_at: null,
        last_activity_at: "2026-11-03T10:00:00.000Z",
        snoozed_until: null,
        snoozed_at: null,
        woke_at: null,
      },
    });
    await vi.waitFor(() =>
      expect(visitOf("PLA-1")).toEqual({
        last_visited_at: "2026-11-03T10:00:00.010Z",
        last_activity_at: "2026-11-03T10:00:00.000Z",
      }),
    );
    expect(markVisited.mock.calls).toEqual([["p1", "PLA-1"]]);
    stop();
  });

  it("applies the visit the backend wrote when marking a task unread", async () => {
    markUnread.mockResolvedValue({
      last_visited_at: "2026-11-03T09:59:59.999Z",
      last_activity_at: "2026-11-03T10:00:00.000Z",
    });

    await taskStore.markTaskUnread("p1", "PLA-2");

    expect(visitOf("PLA-2")).toEqual({
      last_visited_at: "2026-11-03T09:59:59.999Z",
      last_activity_at: "2026-11-03T10:00:00.000Z",
    });
  });

  describe("returning to the window", () => {
    const viewed = (last_activity_at: string, last_visited_at: string | null) => ({
      projectId: "p1",
      task: { ...task("PLA-9"), status: "in_review", last_activity_at, last_visited_at },
    });

    it("keeps a Mark as unread made before leaving", () => {
      vi.useFakeTimers({ now: Date.parse("2026-11-03T10:05:00.000Z") });
      const unreadBeforeLeaving = viewed("2026-11-03T10:00:00.000Z", "2026-11-03T09:59:59.999Z");
      const visit = taskStore.createReturnVisit(() => unreadBeforeLeaving);

      visit.left();
      vi.setSystemTime(Date.parse("2026-11-03T10:10:00.000Z"));
      visit.returned();

      expect(markVisited.mock.calls).toEqual([]);
      vi.useRealTimers();
    });

    it("visits the task when its agent did something while the user was away", () => {
      markVisited.mockResolvedValue({
        last_visited_at: "2026-11-03T10:10:00.000Z",
        last_activity_at: "2026-11-03T10:07:00.000Z",
      });
      vi.useFakeTimers({ now: Date.parse("2026-11-03T10:05:00.000Z") });
      const activityWhileAway = viewed("2026-11-03T10:07:00.000Z", "2026-11-03T10:04:00.000Z");
      const visit = taskStore.createReturnVisit(() => activityWhileAway);

      visit.left();
      vi.setSystemTime(Date.parse("2026-11-03T10:10:00.000Z"));
      visit.returned();

      expect(markVisited.mock.calls).toEqual([["p1", "PLA-9"]]);
      vi.useRealTimers();
    });
  });
});
