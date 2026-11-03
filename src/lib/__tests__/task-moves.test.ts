import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, TaskItem } from "../types";

const { statuses, move } = vi.hoisted(() => ({
  statuses: {} as Record<string, string>,
  move: vi.fn(),
}));

vi.mock("../api", () => ({
  tasks: {
    listAll: () =>
      Promise.resolve(Object.entries(statuses).map(([key, status]) => ({ ...task(key), status }))),
    move,
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import * as taskStore from "../task-store.svelte";
import {
  DONE_UNDO_SECONDS,
  changeTaskStatus,
  commitPendingDone,
  completeTask,
  undoLastTaskMove,
} from "../task-moves";
import { dismissToast, getToasts, requestDismissToast, runToastAction } from "../toasts.svelte";

function task(key: string, status = "in_progress"): TaskItem {
  return {
    key,
    title: `Title of ${key}`,
    status,
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
  };
}

const project: Project = { id: "p", name: "p", path: "/repo", hidden: false, path_missing: false };
const workspace = (key: string) => ({ task: task(key, statusOf(key)), project });
const statusOf = (key: string) =>
  taskStore.getTasksForProject("/repo").find((candidate) => candidate.key === key)?.status;
const messages = () => getToasts().map((toast) => toast.message);
const backendMoves = () => move.mock.calls.map(([key, status]) => `${key}:${status}`);
const waitOutUndoWindow = () => vi.advanceTimersByTimeAsync(DONE_UNDO_SECONDS * 1_000 + 100);

describe("task moves", () => {
  const onMoved = vi.fn();

  beforeEach(async () => {
    vi.useFakeTimers();
    await commitPendingDone();
    await undoLastTaskMove(() => {});
    for (const toast of getToasts()) dismissToast(toast.id);
    onMoved.mockReset();
    for (const key of Object.keys(statuses)) delete statuses[key];
    Object.assign(statuses, { "PLA-1": "in_progress", "PLA-2": "todo" });
    move.mockReset().mockImplementation((key: string, status: string) => {
      statuses[key] = status;
      return Promise.resolve();
    });
    await taskStore.loadTasks(["/repo"]);
    move.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("shows Done at once but only applies it after the undo window", async () => {
    await completeTask(workspace("PLA-1"), onMoved);

    expect(statusOf("PLA-1")).toBe("done");
    expect(backendMoves()).toEqual([]);
    expect(getToasts()).toMatchObject([
      {
        kind: "success",
        message: "PLA-1 moved to Done",
        detail: "Title of PLA-1",
        countdownLabel: "applies in 5s",
        actions: [{ key: "z", label: "Undo" }],
      },
    ]);

    await waitOutUndoWindow();
    expect(backendMoves()).toEqual(["PLA-1:done"]);
    expect(onMoved).toHaveBeenCalledTimes(1);
    expect(statusOf("PLA-1")).toBe("done");
    expect(getToasts()).toEqual([]);
  });

  it("cancels a pending Done with Mod+Z, without touching the backend", async () => {
    await completeTask(workspace("PLA-1"), onMoved);
    await undoLastTaskMove(onMoved);
    expect(messages()).toEqual(["PLA-1 moved back to Running"]);
    await waitOutUndoWindow();

    expect(backendMoves()).toEqual([]);
    expect(statusOf("PLA-1")).toBe("in_progress");
  });

  it("lets the toast's Undo cancel its own Done even after another move", async () => {
    await completeTask(workspace("PLA-1"), onMoved);
    const toastId = getToasts()[0].id;
    await changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);

    runToastAction(toastId, "z");
    await waitOutUndoWindow();

    expect(backendMoves()).toEqual(["PLA-2:in_review"]);
    expect(statusOf("PLA-1")).toBe("in_progress");
    expect(statusOf("PLA-2")).toBe("in_review");
  });

  it("applies a pending Done right away when its toast is dismissed or the app quits", async () => {
    await completeTask(workspace("PLA-1"), onMoved);
    await changeTaskStatus("PLA-2", "done", "/repo", onMoved);

    requestDismissToast(getToasts().find((toast) => toast.message.startsWith("PLA-1"))!.id);
    await vi.waitFor(() => expect(backendMoves()).toEqual(["PLA-1:done"]));
    await commitPendingDone();

    expect(backendMoves()).toEqual(["PLA-1:done", "PLA-2:done"]);
    expect(onMoved).toHaveBeenCalledTimes(2);
  });

  it("does not offer to undo a Done once it has applied", async () => {
    await completeTask(workspace("PLA-1"), onMoved);
    await waitOutUndoWindow();
    await undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-1:done"]);
    expect(statusOf("PLA-1")).toBe("done");
  });

  it("reverts the last applied move with Mod+Z, once", async () => {
    await changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    await undoLastTaskMove(onMoved);
    await undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-2:in_review", "PLA-2:todo"]);
    expect(messages()).toEqual(["PLA-2 moved back to To do"]);
    expect(onMoved).toHaveBeenCalledTimes(2);
  });

  it("leaves a task whose status changed again outside the UI", async () => {
    await changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    statuses["PLA-2"] = "in_progress";
    await taskStore.refresh(["/repo"]);
    await undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-2:in_review"]);
    expect(statusOf("PLA-2")).toBe("in_progress");
  });

  it("sends a reverted reopen back to Done through the undo window", async () => {
    statuses["PLA-2"] = "done";
    await taskStore.refresh(["/repo"]);
    await changeTaskStatus("PLA-2", "todo", "/repo", onMoved);
    await undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-2:todo"]);
    expect(statusOf("PLA-2")).toBe("done");
    await waitOutUndoWindow();
    expect(backendMoves()).toEqual(["PLA-2:todo", "PLA-2:done"]);
  });

  it("keeps a failed revert undoable", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    await changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    move.mockRejectedValueOnce(new Error("database unavailable"));

    await undoLastTaskMove(onMoved);
    expect(messages()).toEqual(["Failed to move task."]);
    await undoLastTaskMove(onMoved);
    expect(statusOf("PLA-2")).toBe("todo");
  });

  it("explains why nothing moved when no task is open or it is already done", async () => {
    statuses["PLA-1"] = "done";
    await taskStore.refresh(["/repo"]);
    await completeTask(null, onMoved);
    await completeTask(workspace("PLA-1"), onMoved);

    expect(backendMoves()).toEqual([]);
    expect(messages()).toEqual(["No task is open.", "PLA-1 is already Done"]);
  });
});
