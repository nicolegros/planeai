/**
 * Pure rules of the Cards sidebar rows: what a row's session is doing, which hover actions it offers,
 * how urgent a collapsed folder is.
 */
import type { Session } from "./types";
import type { ShelfLane, SidebarRow } from "./sidebar-model";
import { isTaskSnoozed, isTaskWoke } from "./task-snooze";
import { isTaskUnread } from "./task-unread";

/**
 * What a row shows in its status slot; a row with nothing new shows nothing. A snoozed row shows its
 * countdown and nothing else.
 */
export type RowActivity = "snoozed" | "woke" | "working" | "unread" | "exited";

export function rowSession(row: SidebarRow): Session | undefined {
  return row.kind === "task" ? row.linkedSession : row.session;
}

/** A task's unread is persisted on the task; a session without a task keeps it in `unreadSessions`. */
export function rowActivity(
  row: SidebarRow,
  agentStates: Record<string, string>,
  unreadSessions: Record<string, boolean>,
  now: number,
): RowActivity | null {
  if (row.kind === "task" && isTaskSnoozed(row.task, now)) return "snoozed";
  if (row.kind === "task" && isTaskWoke(row.task, now)) return "woke";
  const session = rowSession(row);
  if (session && agentStates[session.id] === "Busy") return "working";
  if (row.kind === "task" ? isTaskUnread(row.task) : unreadSessions[row.session.id])
    return "unread";
  if (session?.status === "exited") return "exited";
  return null;
}

export const WOKE_TITLE = "Woke: back from snooze since you last opened it";

/** Nothing in the row needs the user, so it recedes. */
export function isQuiet(activity: RowActivity | null): boolean {
  return activity === null || activity === "exited" || activity === "snoozed";
}

export type RowAction =
  | "seen"
  | "review"
  | "restart"
  | "done"
  | "archive"
  | "start"
  | "reopen"
  | "wake";

/**
 * Hover actions of a row, left to right. `shelf` is the shelf the row sits in, if any. A woken row
 * keeps its Woke pill under the pointer, as the action that marks it seen.
 */
export function rowActions(
  row: SidebarRow,
  shelf?: ShelfLane,
  activity: RowActivity | null = null,
): RowAction[] {
  const actions = shelfActions(row, shelf);
  return activity === "woke" ? ["seen", ...actions] : actions;
}

function shelfActions(row: SidebarRow, shelf?: ShelfLane): RowAction[] {
  if (shelf === "snoozed") return ["wake"];
  if (shelf === "todo") return ["start"];
  if (shelf === "done") return ["reopen"];
  const finish = row.kind === "session" ? "archive" : "done";
  const session = rowSession(row);
  if (!session) return [finish];
  return [session.status === "exited" ? "restart" : "review", finish];
}

/** The status dot of a collapsed folder, most urgent first. */
export type FolderUrgency = "exited" | "review" | "running";

export function folderUrgency(
  rows: SidebarRow[],
  activityOf: (row: SidebarRow) => RowActivity | null,
): FolderUrgency | null {
  let urgency: FolderUrgency | null = null;
  for (const row of rows) {
    const activity = activityOf(row);
    if (activity === "exited") return "exited";
    if (
      activity === "woke" ||
      activity === "unread" ||
      (row.kind === "task" && row.task.status === "in_review")
    ) {
      urgency = "review";
    } else if (activity === "working" && urgency === null) {
      urgency = "running";
    }
  }
  return urgency;
}

/**
 * Splits a branch so that only its head truncates: rendered as a truncating head and a fixed tail,
 * a long branch keeps both ends with an ellipsis in the middle.
 */
export function splitBranch(branch: string, tailLength = 10): [head: string, tail: string] {
  if (branch.length <= tailLength) return ["", branch];
  return [branch.slice(0, -tailLength), branch.slice(-tailLength)];
}
