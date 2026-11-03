import { tasks as tasksApi } from "./api";
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

interface PendingDone {
  move: TaskMove;
  onMoved: () => void;
  hold: number;
}

const pendingDone = new Map<string, PendingDone>();
/** Done moves not yet confirmed applied, including ones being applied or left by an earlier run. */
const savedDone = new Map<string, { key: string; repoPath: string }>();
/** What Mod+Z reverts: a Done still in `pendingDone`, or the last move that applied. */
let lastMove: TaskMove | null = null;

const doneToastId = (id: string) => `task-done:${id}`;

function statusLabel(status: string): string {
  return TASK_STATUS_LABELS[status as TaskStatus] ?? status;
}

function savePendingDone(): void {
  const moves = [...savedDone.values()];
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
  const id = taskStore.taskRef(move.repoPath, move.key);
  const hold = taskStore.holdStatus(move.key, move.repoPath, "done");
  pendingDone.set(id, { move, onMoved, hold });
  savedDone.set(id, { key: move.key, repoPath: move.repoPath });
  savePendingDone();
  lastMove = move;
  showToast({
    id: doneToastId(id),
    kind: "success",
    message: `${move.key} moved to ${statusLabel("done")}`,
    detail: taskStore.findTask(move.repoPath, move.key)?.title,
    actions: [{ key: "z", label: "Undo", run: () => void cancelDone(id) }],
    dismiss: () => commitDone(id),
    countdown: {
      seconds: DONE_UNDO_SECONDS,
      label: (remaining) => `applies in ${remaining}s`,
      onExpire: () => commitDone(id),
    },
  });
}

function takePendingDone(id: string): PendingDone | undefined {
  const pending = pendingDone.get(id);
  if (!pending) return undefined;
  pendingDone.delete(id);
  dismissToast(doneToastId(id));
  if (lastMove === pending.move) lastMove = null;
  return pending;
}

/** `keepIfFailed` leaves a failed move saved for the next launch, for quitting where its error toast can't be seen. */
async function commitDone(id: string, keepIfFailed = false): Promise<void> {
  const pending = takePendingDone(id);
  if (!pending) return;
  const { move, onMoved, hold } = pending;
  const moved = await apply(move.key, "done", move.repoPath);
  taskStore.releaseStatus(move.key, move.repoPath, hold);
  if (moved || !keepIfFailed) {
    savedDone.delete(id);
    savePendingDone();
  }
  if (moved) onMoved();
  else void taskStore.refresh([move.repoPath]);
}

/** Drops a pending Done. The hold hid backend changes meanwhile, so the task is re-read. */
function cancelDone(id: string, announce = true): boolean {
  const pending = takePendingDone(id);
  if (!pending) return false;
  savedDone.delete(id);
  savePendingDone();
  const { move, hold } = pending;
  taskStore.releaseStatus(move.key, move.repoPath, hold, move.from);
  void taskStore.refresh([move.repoPath]);
  if (announce) showMovedBack(move);
  return true;
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
  cancelDone(taskStore.taskRef(repoPath, key), false);
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

/** Applies Done at once for flows with their own confirmation, replacing a pending one. */
export async function applyDoneNow(key: string, repoPath: string): Promise<void> {
  cancelDone(taskStore.taskRef(repoPath, key), false);
  await taskStore.moveTask(key, "done", repoPath);
}

/** Whether Mod+Z has a task move to revert; otherwise the key stays with the focused content. */
export function canUndoTaskMove(): boolean {
  if (!lastMove) return false;
  const { key, repoPath, to } = lastMove;
  return (
    pendingDone.has(taskStore.taskRef(repoPath, key)) ||
    taskStore.findTask(repoPath, key)?.status === to
  );
}

/**
 * Mod+Z: cancels the newest Done still in its undo window, or reverts the last applied move while
 * the task still has the status it was moved to.
 */
export async function undoLastTaskMove(onMoved: () => void): Promise<void> {
  const move = lastMove;
  if (!move) return;
  const id = taskStore.taskRef(move.repoPath, move.key);
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
  await Promise.all([...pendingDone.keys()].map((id) => commitDone(id, true)));
}

/**
 * Applies the Done moves a quit or crash interrupted. Each is checked against the backend, since
 * the store may not have loaded yet; one whose project can't be read stays saved for next time.
 */
export async function applyInterruptedDone(onMoved: () => void): Promise<void> {
  let saved: unknown;
  try {
    saved = JSON.parse(localStorage.getItem(PENDING_DONE_STORAGE_KEY) ?? "[]");
  } catch {
    return;
  }
  if (!Array.isArray(saved)) return;
  const entries = (saved as { key?: unknown; repoPath?: unknown }[]).filter(
    (entry): entry is { key: string; repoPath: string } =>
      typeof entry?.key === "string" && typeof entry.repoPath === "string",
  );
  for (const entry of entries) savedDone.set(taskStore.taskRef(entry.repoPath, entry.key), entry);
  let moved = false;
  for (const { key, repoPath } of entries) {
    const id = taskStore.taskRef(repoPath, key);
    if (pendingDone.has(id)) continue;
    let status: string | undefined;
    try {
      status = (await tasksApi.listAll(repoPath)).find((task) => task.key === key)?.status;
    } catch {
      continue;
    }
    // The user may have started a new Done on it while the project was read.
    if (pendingDone.has(id)) continue;
    if (status !== undefined && status !== "done")
      moved = (await apply(key, "done", repoPath)) || moved;
    savedDone.delete(id);
  }
  savePendingDone();
  if (moved) onMoved();
}
