/**
 * Store for Jira-synced tasks displayed in the dedicated sidebar section.
 */
import { jira } from "./api";
import type { JiraTaskItem } from "./types";

let jiraTasks = $state<JiraTaskItem[]>([]);

export function getJiraTasks(): JiraTaskItem[] {
  return jiraTasks;
}

export async function loadJiraTasks(): Promise<void> {
  try {
    jiraTasks = await jira.listTasks();
  } catch {
    jiraTasks = [];
  }
}

export async function refresh(): Promise<void> {
  await loadJiraTasks();
}
