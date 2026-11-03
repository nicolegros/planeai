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
          results[path] = await tasksApi.listAll(path);
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
          results[path] = await tasksApi.listAll(path);
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

export interface TaskMove {
  key: string;
  repoPath: string;
  from: string;
  to: string;
}

/** The last status change made through `moveTask`, which `undoLastMove` reverts. */
let lastMove: TaskMove | null = null;

function statusOf(key: string, repoPath: string): string | undefined {
  return getTasksForProject(repoPath).find((task) => task.key === key)?.status;
}

export async function moveTask(key: string, status: string, repoPath: string): Promise<void> {
  const from = statusOf(key, repoPath);
  await tasksApi.move(key, status, repoPath);
  if (from !== undefined && from !== status) lastMove = { key, repoPath, from, to: status };
  await loadTasks(Object.keys(tasksByProject));
}

/**
 * Reverts the last move while the task still has the status it was moved to, and only when
 * it moved `key` if one is given. Returns the reverted move, or null when there was none.
 */
export async function undoLastMove(key?: string): Promise<TaskMove | null> {
  const move = lastMove;
  if (!move || (key !== undefined && move.key !== key)) return null;
  lastMove = null;
  if (statusOf(move.key, move.repoPath) !== move.to) return null;
  try {
    await tasksApi.move(move.key, move.from, move.repoPath);
  } catch (error) {
    lastMove = move;
    throw error;
  }
  await loadTasks(Object.keys(tasksByProject));
  return move;
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
