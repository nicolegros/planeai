/**
 * Store for Jira-synced tasks displayed in the dedicated sidebar section.
 */
import { jira } from "./api";
import type { TaskItem } from "./types";

let jiraTasks = $state<TaskItem[]>([]);
let childCounts = $state<Record<string, number>>({});
let requestGeneration = 0;

export function getJiraTasks(): TaskItem[] {
  return jiraTasks;
}

export function getChildCounts(): Record<string, number> {
  return childCounts;
}

export async function loadJiraTasksIfConnected(connected: boolean): Promise<void> {
  if (!connected) return;
  await loadJiraTasks();
}

export async function loadJiraTasks(): Promise<void> {
  const request = ++requestGeneration;
  try {
    const resp = await jira.listTasks();
    if (request !== requestGeneration) return;
    jiraTasks = resp.tasks;
    childCounts = resp.child_counts;
  } catch {
    if (request !== requestGeneration) return;
    jiraTasks = [];
    childCounts = {};
  }
}

export function clearJiraTasks(): void {
  requestGeneration += 1;
  jiraTasks = [];
  childCounts = {};
}
