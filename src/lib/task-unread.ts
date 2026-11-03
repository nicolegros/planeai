import type { TaskVisit } from "./types";

/** An agent finished, asked for attention or exited since the user last looked at the task. */
export function isTaskUnread(task: TaskVisit): boolean {
  if (task.last_activity_at === null) return false;
  if (task.last_visited_at === null) return true;
  return Date.parse(task.last_activity_at) > Date.parse(task.last_visited_at);
}
