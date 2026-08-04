/**
 * Global task store — single source of truth for task state across all components.
 * Replaces independent fetching in TaskPanel, UnifiedSidebar, and App.svelte.
 */
import { tasks as tasksApi } from "./api";
import type { TaskItem } from "./types";

let tasksByProject = $state<Record<string, TaskItem[]>>({});
let loading = $state(false);

export function getTasksByProject(): Record<string, TaskItem[]> {
  return tasksByProject;
}

export function getTasksForProject(path: string): TaskItem[] {
  return tasksByProject[path] ?? [];
}

export function getAllTasks(): TaskItem[] {
  return Object.values(tasksByProject).flat();
}

export function getTaskStatuses(): Record<string, string> {
  const statuses: Record<string, string> = {};
  for (const items of Object.values(tasksByProject)) {
    for (const t of items) statuses[t.key] = t.status;
  }
  return statuses;
}

export function isLoading(): boolean {
  return loading;
}

export async function loadTasks(projectPaths: string[]): Promise<void> {
  if (projectPaths.length === 0) return;
  loading = true;
  try {
    const results: Record<string, TaskItem[]> = {};
    await Promise.all(
      projectPaths.map(async (path) => {
        try {
          results[path] = await tasksApi.listAll(path);
        } catch {
          results[path] = [];
        }
      }),
    );
    tasksByProject = results;
  } finally {
    loading = false;
  }
}

export async function refresh(projectPaths: string[]): Promise<void> {
  await loadTasks(projectPaths);
}

export async function moveTask(key: string, status: string, repoPath: string): Promise<void> {
  await tasksApi.move(key, status, repoPath);
  await loadTasks(Object.keys(tasksByProject));
}

export async function createTask(params: {
  repoPath: string;
  title: string;
  description: string;
  priority: number;
  tags: string[];
  blockedBy: string[];
  parentKey?: string | null;
  baseBranch?: string;
}): Promise<TaskItem> {
  const created = await tasksApi.create(params);
  await loadTasks(Object.keys(tasksByProject));
  return created;
}

export async function editTask(params: {
  repoPath: string;
  key: string;
  title: string;
  description: string;
  priority: number;
  tags: string[] | null;
  blockedBy: string[] | null;
  parentKey?: string | null;
  baseBranch?: string | null;
}): Promise<void> {
  await tasksApi.edit(params);
  await loadTasks(Object.keys(tasksByProject));
}
