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
const heldStatuses = new Map<string, { status: string; token: number }>();
let lastHoldToken = 0;
/** A task's identity across projects, which can reuse keys. */
export const taskRef = (repoPath: string, key: string) => `${repoPath}\u0000${key}`;

/** Applied when a snapshot is assigned, so a hold that starts or ends mid-reload still counts. */
function withHeldStatuses(byProject: Record<string, TaskItem[]>): Record<string, TaskItem[]> {
  if (heldStatuses.size === 0) return byProject;
  return Object.fromEntries(
    Object.entries(byProject).map(([repoPath, items]) => [
      repoPath,
      items.map((task) => {
        const held = heldStatuses.get(taskRef(repoPath, task.key));
        return held === undefined ? task : { ...task, status: held.status };
      }),
    ]),
  );
}

function setLocalStatus(key: string, repoPath: string, status: string): void {
  const items = tasksByProject[repoPath];
  if (!items?.some((task) => task.key === key)) return;
  tasksByProject = {
    ...tasksByProject,
    [repoPath]: items.map((task) => (task.key === key ? { ...task, status } : task)),
  };
}

/** Shows `status` for the task across reloads until its token is released; a newer hold replaces it. */
export function holdStatus(key: string, repoPath: string, status: string): number {
  const token = ++lastHoldToken;
  heldStatuses.set(taskRef(repoPath, key), { status, token });
  setLocalStatus(key, repoPath, status);
  return token;
}

/** Ends a hold unless a newer one replaced it, showing `status` (if given) until the next reload. */
export function releaseStatus(key: string, repoPath: string, token: number, status?: string): void {
  const id = taskRef(repoPath, key);
  if (heldStatuses.get(id)?.token !== token) return;
  heldStatuses.delete(id);
  if (status !== undefined) setLocalStatus(key, repoPath, status);
}

export function getTasksByProject(): Record<string, TaskItem[]> {
  return tasksByProject;
}

export function getTasksForProject(path: string): TaskItem[] {
  return tasksByProject[path] ?? [];
}

export function findTask(repoPath: string, key: string): TaskItem | undefined {
  return getTasksForProject(repoPath).find((task) => task.key === key);
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
    if (requestGeneration === taskRequestGeneration) tasksByProject = withHeldStatuses(results);
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
      tasksByProject = withHeldStatuses({ ...tasksByProject, ...results });
    }
  } finally {
    if (requestGeneration === taskRequestGeneration) loading = false;
  }
}

/** Shows the new status while the backend applies it, so a quick next move or undo starts from it. */
export async function moveTask(key: string, status: string, repoPath: string): Promise<void> {
  const previous = findTask(repoPath, key)?.status;
  const token = holdStatus(key, repoPath, status);
  try {
    await tasksApi.move(key, status, repoPath);
  } catch (error) {
    releaseStatus(key, repoPath, token, previous);
    throw error;
  }
  releaseStatus(key, repoPath, token);
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
