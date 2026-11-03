import { tasks as tasksApi } from "./api";
import { TASK_STATUS_LABELS, type TaskStatus } from "./sidebar-model";
import { isSnoozeActive, isTaskSnoozed, snoozeOf, wakeTimeLabel } from "./task-snooze";
import { getLaneNow } from "./snooze-clock.svelte";
import * as taskStore from "./task-store.svelte";
import { dismissToast, showToast } from "./toasts.svelte";
import type { Project, StoredSnooze, TaskItem, TaskSnooze } from "./types";

/** Done archives the task's sessions and stops their agents, so it waits this long for an undo. */
export const DONE_UNDO_SECONDS = 5;
export const DONE_CANNOT_SNOOZE = "Done tasks cannot be snoozed.";
/** Pending Done moves, so a quit that skips the close handlers (macOS Cmd+Q) or a crash still applies them. */
const PENDING_DONE_STORAGE_KEY = "planeai.pending-done";

interface TaskMove {
  kind: "status";
  key: string;
  repoPath: string;
  from: string;
  to: string;
}

/** A snooze or a wake: the task's snooze columns before and after it. */
interface SnoozeChange {
  kind: "snooze";
  key: string;
  repoPath: string;
  projectId: string;
  before: TaskSnooze;
  /** The write as stored; its revision tells whether the task still shows it. */
  after: StoredSnooze;
}

/** A task change made in the UI that Mod+Z can revert. */
type TaskChange = TaskMove | SnoozeChange;

interface PendingDone {
  move: TaskMove;
  onMoved: () => void;
  hold: number;
}

const pendingDone = new Map<string, PendingDone>();
/** Done moves not yet confirmed applied, including ones being applied or left by an earlier run. */
const savedDone = new Map<string, { key: string; repoPath: string }>();
/** What Mod+Z reverts: a Done still in `pendingDone`, or the last change that applied. */
let lastChange: TaskChange | null = null;

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
  lastChange = move;
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
  if (lastChange === pending.move) lastChange = null;
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
  const id = taskStore.taskRef(repoPath, key);
  cancelDone(id, false);
  if (savedDone.delete(id)) savePendingDone();
  const from = taskStore.findTask(repoPath, key)?.status;
  if (from === status) return true;
  if (from !== undefined && status === "done") {
    startPendingDone({ kind: "status", key, repoPath, from, to: status }, onMoved);
    return true;
  }
  if (!(await apply(key, status, repoPath))) return false;
  if (from !== undefined) lastChange = { kind: "status", key, repoPath, from, to: status };
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

/** The task still shows the change, so reverting it undoes what the user did. */
function stands(change: TaskChange): boolean {
  const task = taskStore.findTask(change.repoPath, change.key);
  switch (change.kind) {
    case "status":
      return (
        pendingDone.has(taskStore.taskRef(change.repoPath, change.key)) ||
        task?.status === change.to
      );
    case "snooze":
      return task?.snooze_rev === change.after.snooze_rev;
  }
}

async function applyRevert(change: TaskChange, onMoved: () => void): Promise<boolean> {
  switch (change.kind) {
    case "status": {
      const id = taskStore.taskRef(change.repoPath, change.key);
      if (pendingDone.get(id)?.move === change) return cancelDone(id);
      if (change.from === "done") {
        startPendingDone({ ...change, from: change.to, to: "done" }, onMoved);
        return true;
      }
      if (!(await apply(change.key, change.from, change.repoPath))) return false;
      onMoved();
      showMovedBack(change);
      return true;
    }
    case "snooze": {
      try {
        await taskStore.restoreTaskSnooze(change.projectId, change.key, change.before);
      } catch (error) {
        showToast(`Failed to undo: ${error}`, "error");
        return false;
      }
      const { snoozed_until } = change.before;
      const message = isSnoozeActive(change.before, Date.now())
        ? `${change.key} snoozed until ${wakeTimeLabel(Date.parse(snoozed_until!))}`
        : `${change.key} is no longer snoozed`;
      showToast({ kind: "info", message, autoDismissMs: 3000 });
      return true;
    }
  }
}

/** Reverts `change` once, while the task still shows it. */
async function revert(change: TaskChange, onMoved: () => void): Promise<void> {
  if (lastChange === change) lastChange = null;
  if (!stands(change)) return;
  if (!(await applyRevert(change, onMoved)) && lastChange === null) lastChange = change;
}

/** Whether Mod+Z has a task change to revert; otherwise the key stays with the focused content. */
export function canUndoTaskChange(): boolean {
  return lastChange !== null && stands(lastChange);
}

/**
 * Mod+Z: undoes the last UI task change, once: a status move, a snooze or a wake. A Done still in
 * its undo window is cancelled; any other change is reverted while the task still shows it.
 */
export async function undoLastTaskChange(onMoved: () => void): Promise<void> {
  if (lastChange) await revert(lastChange, onMoved);
}

const SNOOZE_TOAST_MS = 5000;

async function changeSnooze(
  task: TaskItem,
  project: Project,
  write: () => Promise<StoredSnooze>,
  message: string,
  detail?: string,
): Promise<boolean> {
  const before = snoozeOf(task);
  let after: StoredSnooze;
  try {
    after = await write();
  } catch (error) {
    showToast(`Failed to update ${task.key}: ${error}`, "error");
    return false;
  }
  const change: SnoozeChange = {
    kind: "snooze",
    key: task.key,
    repoPath: project.path,
    projectId: project.id,
    before,
    after,
  };
  lastChange = change;
  showToast({
    kind: "success",
    message,
    detail,
    actions: [{ key: "z", label: "Undo", run: () => revert(change, () => {}) }],
    autoDismissMs: SNOOZE_TOAST_MS,
  });
  return true;
}

/** Snoozes the task until `until`, which the picker resolved at the moment of the click. */
export async function snoozeTask(task: TaskItem, project: Project, until: Date): Promise<boolean> {
  if (task.status === "done") {
    showToast({ kind: "info", message: DONE_CANNOT_SNOOZE, autoDismissMs: 3000 });
    return false;
  }
  if (until.getTime() <= Date.now()) {
    showToast({ kind: "info", message: "That snooze time has passed", autoDismissMs: 3000 });
    return false;
  }
  return changeSnooze(
    task,
    project,
    () => taskStore.snoozeTask(project.id, task.key, until),
    `Snoozed until ${wakeTimeLabel(until.getTime())}`,
    task.title,
  );
}

/** Wakes the task now; the user acted, so it leaves no Woke marker. */
export async function wakeTask(task: TaskItem, project: Project): Promise<boolean> {
  if (!isTaskSnoozed(task, getLaneNow())) return false;
  return changeSnooze(
    task,
    project,
    () => taskStore.wakeTask(project.id, task.key),
    `Woke ${task.title}`,
  );
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
    // The user may have changed its status while the project was read; that choice wins.
    if (pendingDone.has(id) || !savedDone.has(id)) continue;
    if (status !== undefined && status !== "done")
      moved = (await apply(key, "done", repoPath)) || moved;
    savedDone.delete(id);
  }
  savePendingDone();
  if (moved) onMoved();
}
