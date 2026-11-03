import type { TaskItem } from "./types";

/**
 * An agent finished, asked for attention or exited since the user last looked at the task.
 * A Done task is finished business, so what happened before it is never unread.
 */
export function isTaskUnread(
  task: Pick<TaskItem, "status" | "last_visited_at" | "last_activity_at">,
): boolean {
  if (task.status === "done" || task.last_activity_at === null) return false;
  if (task.last_visited_at === null) return true;
  return Date.parse(task.last_activity_at) > Date.parse(task.last_visited_at);
}
