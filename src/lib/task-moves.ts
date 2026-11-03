import { TASK_STATUS_LABELS, type TaskStatus } from "./sidebar-model";
import * as taskStore from "./task-store.svelte";
import { dismissToast, showToast } from "./toasts.svelte";
import type { Project, TaskItem } from "./types";

/** Done archives the task's sessions and stops their agents, so it waits this long for an undo. */
export const DONE_UNDO_SECONDS = 5;

interface TaskMove {
  key: string;
  repoPath: string;
  from: string;
  to: string;
}

const pendingDone = new Map<string, { move: TaskMove; onMoved: () => void }>();
/** What Mod+Z reverts: the newest Done still in its undo window, or the last move that applied. */
let lastAction: { move: TaskMove; pending: boolean } | null = null;

const doneToastId = (key: string) => `task-done:${key}`;

function statusLabel(status: string): string {
  return TASK_STATUS_LABELS[status as TaskStatus] ?? status;
}

function findTask(key: string, repoPath: string): TaskItem | undefined {
  return taskStore.getTasksForProject(repoPath).find((task) => task.key === key);
}

async function apply(key: string, status: string, repoPath: string): Promise<boolean> {
  try {
    await taskStore.moveTask(key, status, repoPath);
    return true;
  } catch (error) {
    console.error("Failed to move task:", error);
    showToast("Failed to move task.");
    return false;
  }
}

function showMovedBack(move: TaskMove): void {
  showToast({
    kind: "info",
    message: `${move.key} moved back to ${statusLabel(move.from)}`,
    autoDismissMs: 3000,
  });
}

function startPendingDone(move: TaskMove, onMoved: () => void): void {
  taskStore.holdStatus(move.key, move.repoPath, "done");
  pendingDone.set(move.key, { move, onMoved });
  lastAction = { move, pending: true };
  showToast({
    id: doneToastId(move.key),
    kind: "success",
    message: `${move.key} moved to ${statusLabel("done")}`,
    detail: findTask(move.key, move.repoPath)?.title,
    actions: [{ key: "z", label: "Undo", run: () => cancelDone(move.key) }],
    dismiss: () => commitDone(move.key),
    countdown: {
      seconds: DONE_UNDO_SECONDS,
      label: (remaining) => `applies in ${remaining}s`,
      onExpire: () => commitDone(move.key),
    },
  });
}

function takePendingDone(key: string): { move: TaskMove; onMoved: () => void } | undefined {
  const pending = pendingDone.get(key);
  if (!pending) return undefined;
  pendingDone.delete(key);
  dismissToast(doneToastId(key));
  if (lastAction?.move === pending.move) lastAction = null;
  return pending;
}

async function commitDone(key: string): Promise<void> {
  const pending = takePendingDone(key);
  if (!pending) return;
  const { move, onMoved } = pending;
  const moved = await apply(move.key, "done", move.repoPath);
  taskStore.releaseStatus(move.key, move.repoPath, moved ? "done" : move.from);
  if (moved) onMoved();
}

function cancelDone(key: string): void {
  const pending = takePendingDone(key);
  if (!pending) return;
  taskStore.releaseStatus(key, pending.move.repoPath, pending.move.from);
  showMovedBack(pending.move);
}

/**
 * Changes a task's status from the UI, telling the user when it fails. A move to Done waits out
 * its undo window first. `onMoved` runs once the backend has the new status.
 */
export async function changeTaskStatus(
  key: string,
  status: string,
  repoPath: string,
  onMoved: () => void,
): Promise<boolean> {
  const superseded = takePendingDone(key);
  if (superseded) taskStore.releaseStatus(key, repoPath, superseded.move.from);
  const from = findTask(key, repoPath)?.status;
  if (from === status) return true;
  if (from !== undefined && status === "done") {
    startPendingDone({ key, repoPath, from, to: status }, onMoved);
    return true;
  }
  if (!(await apply(key, status, repoPath))) return false;
  if (from !== undefined)
    lastAction = { move: { key, repoPath, from, to: status }, pending: false };
  onMoved();
  return true;
}

/** Mod+Option+S: moves the open task to Done. */
export async function completeTask(
  workspace: { task: TaskItem; project: Project } | null,
  onMoved: () => void,
): Promise<void> {
  if (!workspace) {
    showToast({ kind: "info", message: "No task is open.", autoDismissMs: 3000 });
    return;
  }
  const { task, project } = workspace;
  if (task.status === "done") {
    const message = `${task.key} is already ${statusLabel("done")}`;
    showToast({ kind: "info", message, autoDismissMs: 3000 });
    return;
  }
  await changeTaskStatus(task.key, "done", project.path, onMoved);
}

/**
 * Mod+Z: cancels the newest Done still in its undo window, or reverts the last applied move while
 * the task still has the status it was moved to.
 */
export async function undoLastTaskMove(onMoved: () => void): Promise<void> {
  const action = lastAction;
  if (!action) return;
  const { move } = action;
  if (action.pending) {
    cancelDone(move.key);
    return;
  }
  lastAction = null;
  if (findTask(move.key, move.repoPath)?.status !== move.to) return;
  if (move.from === "done") {
    startPendingDone(
      { key: move.key, repoPath: move.repoPath, from: move.to, to: "done" },
      onMoved,
    );
    return;
  }
  if (await apply(move.key, move.from, move.repoPath)) {
    onMoved();
    showMovedBack(move);
  } else if (lastAction === null) {
    lastAction = action;
  }
}

/** Applies every Done still in its undo window, for quitting. */
export async function commitPendingDone(): Promise<void> {
  await Promise.all([...pendingDone.keys()].map(commitDone));
}
