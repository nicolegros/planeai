/**
 * Pure rules of the Cards sidebar rows: what a row's session is doing, which hover actions it offers,
 * how urgent a collapsed folder is.
 */
import type { Session } from "./types";
import type { ShelfStatus, SidebarRow } from "./sidebar-model";

/** What a row's session shows in its status slot; idle and session-less rows show nothing. */
export type SessionActivity = "working" | "ready" | "exited";

export function rowSession(row: SidebarRow): Session | undefined {
  return row.kind === "task" ? row.linkedSession : row.session;
}

export function sessionActivity(
  session: Session | undefined,
  agentStates: Record<string, string>,
  reviewReady: Record<string, boolean>,
): SessionActivity | null {
  if (!session) return null;
  if (agentStates[session.id] === "Busy") return "working";
  if (reviewReady[session.id]) return "ready";
  if (session.status === "exited") return "exited";
  return null;
}

export type RowAction = "review" | "restart" | "done" | "start" | "reopen";

/** Hover actions of a row, left to right. `shelf` is the shelf the row sits in, if any. */
export function rowActions(row: SidebarRow, shelf?: ShelfStatus): RowAction[] {
  if (shelf === "todo") return ["start"];
  if (shelf === "done") return ["reopen"];
  const session = rowSession(row);
  if (!session) return ["done"];
  return [session.status === "exited" ? "restart" : "review", "done"];
}

/** The status dot of a collapsed folder, most urgent first. */
export type FolderUrgency = "exited" | "review" | "running";

export function folderUrgency(
  rows: SidebarRow[],
  activityOf: (row: SidebarRow) => SessionActivity | null,
): FolderUrgency | null {
  let urgency: FolderUrgency | null = null;
  for (const row of rows) {
    const activity = activityOf(row);
    if (activity === "exited") return "exited";
    if (activity === "ready" || (row.kind === "task" && row.task.status === "in_review")) {
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
