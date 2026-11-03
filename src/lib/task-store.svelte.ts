/**
 * Global task store — single source of truth for task state across all components.
 * Replaces independent fetching in UnifiedSidebar and App.svelte.
 */
import { listen } from "@tauri-apps/api/event";
import { tasks as tasksApi } from "./api";
import type { TaskItem } from "./types";

let tasksByProject = $state<Record<string, TaskItem[]>>({});
let loading = $state(false);
let taskRequestGeneration = 0;

/** Statuses shown ahead of the backend, such as a Done move waiting out its undo window. */
const heldStatuses = new Map<string, string>();
const heldKey = (repoPath: string, key: string) => `${repoPath}\u0000${key}`;

function withHeldStatuses(repoPath: string, items: TaskItem[]): TaskItem[] {
  if (heldStatuses.size === 0) return items;
  return items.map((task) => {
    const status = heldStatuses.get(heldKey(repoPath, task.key));
    return status === undefined ? task : { ...task, status };
  });
}

function setLocalStatus(key: string, repoPath: string, status: string): void {
  const items = tasksByProject[repoPath];
  if (!items?.some((task) => task.key === key)) return;
  tasksByProject = {
    ...tasksByProject,
    [repoPath]: items.map((task) => (task.key === key ? { ...task, status } : task)),
  };
}

/** Shows `status` for the task until `releaseStatus`, surviving reloads. */
export function holdStatus(key: string, repoPath: string, status: string): void {
  heldStatuses.set(heldKey(repoPath, key), status);
  setLocalStatus(key, repoPath, status);
}

/** Ends a hold, showing `status` until the next reload brings the backend's. */
export function releaseStatus(key: string, repoPath: string, status: string): void {
  heldStatuses.delete(heldKey(repoPath, key));
  setLocalStatus(key, repoPath, status);
}

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

/** Replaces the full store snapshot, retaining prior data for failed project requests. */
export async function loadTasks(projectPaths: string[]): Promise<void> {
  const requestGeneration = ++taskRequestGeneration;
  loading = true;
  try {
    if (projectPaths.length === 0) {
      tasksByProject = {};
      return;
    }
    const previous = tasksByProject;
    const results: Record<string, TaskItem[]> = {};
    await Promise.all(
      projectPaths.map(async (path) => {
        try {
          results[path] = withHeldStatuses(path, await tasksApi.listAll(path));
        } catch {
          results[path] = previous[path] ?? [];
        }
      }),
    );
    if (requestGeneration === taskRequestGeneration) tasksByProject = results;
  } finally {
    if (requestGeneration === taskRequestGeneration) loading = false;
  }
}

/** Refreshes on `tasks-changed`, emitted for status moves made outside the UI (lifecycle hooks, CLI); bursts are coalesced. */
export function startTaskEventListener(getProjectPaths: () => string[]): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const unlisten = listen("tasks-changed", () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      void refresh(getProjectPaths());
    }, 150);
  });
  return () => {
    if (timer) clearTimeout(timer);
    void unlisten.then((fn) => fn());
  };
}

/** Refreshes only the supplied projects, preserving unrelated store entries. */
export async function refresh(projectPaths: string[]): Promise<void> {
  const requestGeneration = ++taskRequestGeneration;
  loading = true;
  try {
    if (projectPaths.length === 0) {
      tasksByProject = {};
      return;
    }
    const results: Record<string, TaskItem[]> = {};
    await Promise.all(
      projectPaths.map(async (path) => {
        try {
          results[path] = withHeldStatuses(path, await tasksApi.listAll(path));
        } catch {
          // Keep the last successful snapshot for this project on refresh failure.
        }
      }),
    );
    if (requestGeneration === taskRequestGeneration) {
      tasksByProject = { ...tasksByProject, ...results };
    }
  } finally {
    if (requestGeneration === taskRequestGeneration) loading = false;
  }
}

export async function moveTask(key: string, status: string, repoPath: string): Promise<void> {
  await tasksApi.move(key, status, repoPath);
  // Show the move before the reload lands, so an immediate undo sees it.
  setLocalStatus(key, repoPath, status);
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
  // Refresh store in background — don't block return on refresh failure
  loadTasks(Object.keys(tasksByProject)).catch(() => {});
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
