import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, TaskItem } from "../types";

const { statuses, move, listAllFails } = vi.hoisted(() => ({
  statuses: {} as Record<string, string>,
  move: vi.fn(),
  listAllFails: { value: false },
}));

vi.mock("../api", () => ({
  tasks: {
    listAll: () =>
      listAllFails.value
        ? Promise.reject(new Error("project unavailable"))
        : Promise.resolve(
            Object.entries(statuses).map(([key, status]) => ({ ...task(key), status })),
          ),
    move,
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

type TaskStore = typeof import("../task-store.svelte");
type TaskMoves = typeof import("../task-moves");
type Toasts = typeof import("../toasts.svelte");
let taskStore: TaskStore;
let moves: TaskMoves;
let toasts: Toasts;

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
const statusOf = (key: string, repoPath = "/repo") => taskStore.findTask(repoPath, key)?.status;
const messages = () => toasts.getToasts().map((toast) => toast.message);
const backendMoves = () => move.mock.calls.map(([key, status]) => `${key}:${status}`);
const waitOutUndoWindow = () => vi.advanceTimersByTimeAsync(moves.DONE_UNDO_SECONDS * 1_000 + 100);

describe("task moves", () => {
  const onMoved = vi.fn();

  beforeEach(async () => {
    vi.useFakeTimers();
    vi.resetModules();
    localStorage.clear();
    listAllFails.value = false;
    [taskStore, moves, toasts] = await Promise.all([
      import("../task-store.svelte"),
      import("../task-moves"),
      import("../toasts.svelte"),
    ]);
    onMoved.mockReset();
    for (const key of Object.keys(statuses)) delete statuses[key];
    Object.assign(statuses, { "PLA-1": "in_progress", "PLA-2": "todo" });
    move.mockReset().mockImplementation((key: string, status: string) => {
      statuses[key] = status;
      return Promise.resolve();
    });
    await taskStore.loadTasks(["/repo"]);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("shows Done at once but only applies it after the undo window", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);

    expect(statusOf("PLA-1")).toBe("done");
    expect(backendMoves()).toEqual([]);
    expect(toasts.getToasts()).toMatchObject([
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
    expect(toasts.getToasts()).toEqual([]);
  });

  it("cancels a pending Done with Mod+Z, without touching the backend", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    await moves.undoLastTaskMove(onMoved);
    expect(messages()).toEqual(["PLA-1 moved back to Running"]);
    await waitOutUndoWindow();

    expect(backendMoves()).toEqual([]);
    expect(statusOf("PLA-1")).toBe("in_progress");
  });

  it("lets the toast's Undo cancel its own Done even after another move", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    const toastId = toasts.getToasts()[0].id;
    await moves.changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);

    toasts.runToastAction(toastId, "z");
    await waitOutUndoWindow();

    expect(backendMoves()).toEqual(["PLA-2:in_review"]);
    expect(statusOf("PLA-1")).toBe("in_progress");
    expect(statusOf("PLA-2")).toBe("in_review");
  });

  it("applies a pending Done right away when its toast is dismissed or the app quits", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    await moves.changeTaskStatus("PLA-2", "done", "/repo", onMoved);

    toasts.requestDismissToast(
      toasts.getToasts().find((toast) => toast.message.startsWith("PLA-1"))!.id,
    );
    await vi.waitFor(() => expect(backendMoves()).toEqual(["PLA-1:done"]));
    await moves.commitPendingDone();

    expect(backendMoves()).toEqual(["PLA-1:done", "PLA-2:done"]);
    expect(onMoved).toHaveBeenCalledTimes(2);
  });

  it("does not offer to undo a Done once it has applied", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    await waitOutUndoWindow();
    await moves.undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-1:done"]);
    expect(statusOf("PLA-1")).toBe("done");
  });

  it("reverts the last applied move with Mod+Z, once", async () => {
    await moves.changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    await moves.undoLastTaskMove(onMoved);
    await moves.undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-2:in_review", "PLA-2:todo"]);
    expect(messages()).toEqual(["PLA-2 moved back to To do"]);
    expect(onMoved).toHaveBeenCalledTimes(2);
  });

  it("leaves a task whose status changed again outside the UI", async () => {
    await moves.changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    statuses["PLA-2"] = "in_progress";
    await taskStore.refresh(["/repo"]);
    expect(moves.canUndoTaskMove()).toBe(false);
    await moves.undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-2:in_review"]);
    expect(statusOf("PLA-2")).toBe("in_progress");
  });

  it("sends a reverted reopen back to Done through the undo window", async () => {
    statuses["PLA-2"] = "done";
    await taskStore.refresh(["/repo"]);
    await moves.changeTaskStatus("PLA-2", "todo", "/repo", onMoved);
    await moves.undoLastTaskMove(onMoved);

    expect(backendMoves()).toEqual(["PLA-2:todo"]);
    expect(statusOf("PLA-2")).toBe("done");
    await waitOutUndoWindow();
    expect(backendMoves()).toEqual(["PLA-2:todo", "PLA-2:done"]);
  });

  it("keeps a failed revert undoable", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    await moves.changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    move.mockRejectedValueOnce(new Error("database unavailable"));

    await moves.undoLastTaskMove(onMoved);
    expect(messages()).toEqual(["Failed to move task."]);
    await moves.undoLastTaskMove(onMoved);
    expect(statusOf("PLA-2")).toBe("todo");
  });

  it("explains why nothing moved when no task is open or it is already done", async () => {
    statuses["PLA-1"] = "done";
    await taskStore.refresh(["/repo"]);
    await moves.completeTask(null, onMoved);
    await moves.completeTask(workspace("PLA-1"), onMoved);

    expect(backendMoves()).toEqual([]);
    expect(messages()).toEqual(["No task is open.", "PLA-1 is already Done"]);
  });

  it("keeps same-key tasks of different projects apart", async () => {
    statuses["PLA-1"] = "in_progress";
    await taskStore.loadTasks(["/repo", "/other"]);
    await moves.changeTaskStatus("PLA-1", "done", "/repo", onMoved);
    await moves.changeTaskStatus("PLA-1", "in_review", "/other", onMoved);

    expect(statusOf("PLA-1", "/repo")).toBe("done");
    await waitOutUndoWindow();
    expect(backendMoves()).toEqual(["PLA-1:in_review", "PLA-1:done"]);
  });

  it("says whether Mod+Z has a move to revert", async () => {
    expect(moves.canUndoTaskMove()).toBe(false);
    await moves.changeTaskStatus("PLA-2", "in_review", "/repo", onMoved);
    expect(moves.canUndoTaskMove()).toBe(true);
    await moves.undoLastTaskMove(onMoved);
    expect(moves.canUndoTaskMove()).toBe(false);
  });

  it("applies a Done that a quit interrupted on the next launch, once", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    expect(JSON.parse(localStorage.getItem("planeai.pending-done")!)).toEqual([
      { key: "PLA-1", repoPath: "/repo" },
    ]);

    vi.resetModules();
    [taskStore, moves] = await Promise.all([
      import("../task-store.svelte"),
      import("../task-moves"),
    ]);
    await taskStore.loadTasks(["/repo"]);
    onMoved.mockReset();
    await moves.applyInterruptedDone(onMoved);
    await moves.applyInterruptedDone(onMoved);

    expect(backendMoves()).toEqual(["PLA-1:done"]);
    expect(onMoved).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem("planeai.pending-done")).toBeNull();
  });

  it("forgets a pending Done once it is cancelled or applied", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    await moves.undoLastTaskMove(onMoved);
    expect(localStorage.getItem("planeai.pending-done")).toBeNull();

    await moves.completeTask(workspace("PLA-1"), onMoved);
    await waitOutUndoWindow();
    expect(localStorage.getItem("planeai.pending-done")).toBeNull();
  });

  it("replaces a pending Done when the task moves elsewhere first", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    await moves.changeTaskStatus("PLA-1", "in_review", "/repo", onMoved);
    await waitOutUndoWindow();

    expect(backendMoves()).toEqual(["PLA-1:in_review"]);
    expect(statusOf("PLA-1")).toBe("in_review");
    expect(messages()).toEqual([]);
    expect(localStorage.getItem("planeai.pending-done")).toBeNull();
  });

  it("applies Done at once for a flow with its own confirmation, replacing a pending one", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    await moves.applyDoneNow("PLA-1", "/repo");
    await waitOutUndoWindow();

    expect(backendMoves()).toEqual(["PLA-1:done"]);
    expect(messages()).toEqual([]);
    expect(moves.canUndoTaskMove()).toBe(false);
  });

  it("shows the backend's status after an undo, not the one the hold hid", async () => {
    await moves.completeTask(workspace("PLA-1"), onMoved);
    statuses["PLA-1"] = "in_review";
    await taskStore.refresh(["/repo"]);
    expect(statusOf("PLA-1")).toBe("done");

    await moves.undoLastTaskMove(onMoved);
    await vi.waitFor(() => expect(statusOf("PLA-1")).toBe("in_review"));
  });

  it("keeps an interrupted Done saved while its project can't be read", async () => {
    localStorage.setItem(
      "planeai.pending-done",
      JSON.stringify([{ key: "PLA-1", repoPath: "/repo" }]),
    );
    listAllFails.value = true;
    await moves.applyInterruptedDone(onMoved);
    expect(backendMoves()).toEqual([]);
    expect(JSON.parse(localStorage.getItem("planeai.pending-done")!)).toEqual([
      { key: "PLA-1", repoPath: "/repo" },
    ]);

    listAllFails.value = false;
    await moves.applyInterruptedDone(onMoved);
    expect(backendMoves()).toEqual(["PLA-1:done"]);
    expect(localStorage.getItem("planeai.pending-done")).toBeNull();
  });

  it("keeps a Done that failed to apply while quitting saved for the next launch", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    await moves.completeTask(workspace("PLA-1"), onMoved);
    move.mockRejectedValueOnce(new Error("database busy"));
    await moves.commitPendingDone();

    expect(JSON.parse(localStorage.getItem("planeai.pending-done")!)).toEqual([
      { key: "PLA-1", repoPath: "/repo" },
    ]);
  });
});
