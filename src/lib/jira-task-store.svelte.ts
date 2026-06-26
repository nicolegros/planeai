/**
 * Store for Jira-synced tasks displayed in the dedicated sidebar section.
 */
import { jira } from "./api";
import type { TaskItem } from "./types";

let jiraTasks = $state<TaskItem[]>([]);
let childCounts = $state<Record<string, number>>({});

export function getJiraTasks(): TaskItem[] {
  return jiraTasks;
}

export function getChildCounts(): Record<string, number> {
  return childCounts;
}

export async function loadJiraTasks(): Promise<void> {
  try {
    const resp = await jira.listTasks();
    jiraTasks = resp.tasks;
    childCounts = resp.child_counts;
  } catch {
    jiraTasks = [];
    childCounts = {};
  }
}
