import { TASK_STATUS_LABELS, type TaskStatus } from "./sidebar-model";
import * as taskStore from "./task-store.svelte";
import { dismissToast, showToast } from "./toasts.svelte";
import type { Project, TaskItem } from "./types";

/** Done archives the task's sessions and stops their agents, so it waits this long for an undo. */
export const DONE_UNDO_SECONDS = 5;
/** Pending Done moves, so a quit that skips the close handlers (macOS Cmd+Q) or a crash still applies them. */
const PENDING_DONE_STORAGE_KEY = "planeai.pending-done";

interface TaskMove {
  key: string;
  repoPath: string;
  from: string;
  to: string;
}

const pendingDone = new Map<string, { move: TaskMove; onMoved: () => void }>();
/** What Mod+Z reverts: a Done still in `pendingDone`, or the last move that applied. */
let lastMove: TaskMove | null = null;

const taskId = (repoPath: string, key: string) => `${repoPath}\u0000${key}`;
const doneToastId = (id: string) => `task-done:${id}`;

function statusLabel(status: string): string {
  return TASK_STATUS_LABELS[status as TaskStatus] ?? status;
}

function savePendingDone(): void {
  const moves = [...pendingDone.values()].map(({ move }) => ({
    key: move.key,
    repoPath: move.repoPath,
  }));
  try {
    if (moves.length > 0) localStorage.setItem(PENDING_DONE_STORAGE_KEY, JSON.stringify(moves));
    else localStorage.removeItem(PENDING_DONE_STORAGE_KEY);
  } catch {
    // Without storage, an interrupted Done is lost; the window close handler still applies it.
  }
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
  const id = taskId(move.repoPath, move.key);
  taskStore.holdStatus(move.key, move.repoPath, "done");
  pendingDone.set(id, { move, onMoved });
  savePendingDone();
  lastMove = move;
  showToast({
    id: doneToastId(id),
    kind: "success",
    message: `${move.key} moved to ${statusLabel("done")}`,
    detail: taskStore.findTask(move.repoPath, move.key)?.title,
    actions: [{ key: "z", label: "Undo", run: () => cancelDone(id) }],
    dismiss: () => commitDone(id),
    countdown: {
      seconds: DONE_UNDO_SECONDS,
      label: (remaining) => `applies in ${remaining}s`,
      onExpire: () => commitDone(id),
    },
  });
}

function takePendingDone(id: string): { move: TaskMove; onMoved: () => void } | undefined {
  const pending = pendingDone.get(id);
  if (!pending) return undefined;
  pendingDone.delete(id);
  dismissToast(doneToastId(id));
  if (lastMove === pending.move) lastMove = null;
  return pending;
}

async function commitDone(id: string): Promise<void> {
  const pending = takePendingDone(id);
  if (!pending) return;
  const { move, onMoved } = pending;
  const moved = await apply(move.key, "done", move.repoPath);
  // Saved only now, so a quit while this applies still finds it on the next launch.
  savePendingDone();
  taskStore.releaseStatus(move.key, move.repoPath, moved ? "done" : move.from);
  if (moved) onMoved();
}

function cancelDone(id: string): void {
  const pending = takePendingDone(id);
  if (!pending) return;
  savePendingDone();
  const { move } = pending;
  taskStore.releaseStatus(move.key, move.repoPath, move.from);
  showMovedBack(move);
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
  const superseded = takePendingDone(taskId(repoPath, key));
  if (superseded) {
    savePendingDone();
    taskStore.releaseStatus(key, repoPath, superseded.move.from);
  }
  const from = taskStore.findTask(repoPath, key)?.status;
  if (from === status) return true;
  if (from !== undefined && status === "done") {
    startPendingDone({ key, repoPath, from, to: status }, onMoved);
    return true;
  }
  if (!(await apply(key, status, repoPath))) return false;
  if (from !== undefined) lastMove = { key, repoPath, from, to: status };
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

/** Whether Mod+Z has a task move to revert; otherwise the key stays with the focused content. */
export function canUndoTaskMove(): boolean {
  return lastMove !== null;
}

/**
 * Mod+Z: cancels the newest Done still in its undo window, or reverts the last applied move while
 * the task still has the status it was moved to.
 */
export async function undoLastTaskMove(onMoved: () => void): Promise<void> {
  const move = lastMove;
  if (!move) return;
  const id = taskId(move.repoPath, move.key);
  if (pendingDone.get(id)?.move === move) {
    cancelDone(id);
    return;
  }
  lastMove = null;
  if (taskStore.findTask(move.repoPath, move.key)?.status !== move.to) return;
  if (move.from === "done") {
    startPendingDone({ ...move, from: move.to, to: "done" }, onMoved);
    return;
  }
  if (await apply(move.key, move.from, move.repoPath)) {
    onMoved();
    showMovedBack(move);
  } else if (lastMove === null) {
    lastMove = move;
  }
}

/** Applies every Done still in its undo window, for quitting. */
export async function commitPendingDone(): Promise<void> {
  await Promise.all([...pendingDone.keys()].map(commitDone));
}

/** Applies the Done moves a quit or crash interrupted, once the tasks have loaded. */
export async function applyInterruptedDone(): Promise<void> {
  let saved: unknown;
  try {
    saved = JSON.parse(localStorage.getItem(PENDING_DONE_STORAGE_KEY) ?? "[]");
    localStorage.removeItem(PENDING_DONE_STORAGE_KEY);
  } catch {
    return;
  }
  if (!Array.isArray(saved)) return;
  for (const entry of saved as { key?: unknown; repoPath?: unknown }[]) {
    if (typeof entry?.key !== "string" || typeof entry.repoPath !== "string") continue;
    const task = taskStore.findTask(entry.repoPath, entry.key);
    if (task && task.status !== "done") await apply(entry.key, "done", entry.repoPath);
  }
}
