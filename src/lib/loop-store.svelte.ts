/**
 * Loop store — reactive state for loop runs, with event-driven refresh.
 *
 * Listens for `loop-state-changed` events from the backend and refreshes.
 * Also supports manual refresh for the active loop detail view.
 */

import { listen } from "@tauri-apps/api/event";
import { loops as loopsApi } from "./api";
import type { LoopRunSummary } from "./types";

// ─── State ───────────────────────────────────────────────────────────────────

let loopsByProject = $state<Record<string, LoopRunSummary[]>>({});
let activeLoopId = $state<string | null>(null);
let unlistenFn: (() => void) | null = null;

// ─── Getters ─────────────────────────────────────────────────────────────────

export function getLoopsForProject(projectId: string): LoopRunSummary[] {
  return loopsByProject[projectId] ?? [];
}

export function getActiveLoopId(): string | null {
  return activeLoopId;
}

// ─── Actions ─────────────────────────────────────────────────────────────────

export function setActiveLoopId(id: string | null) {
  activeLoopId = id;
}

export async function refreshLoopsForProject(projectId: string): Promise<void> {
  try {
    const runs = await loopsApi.list(projectId);
    loopsByProject = { ...loopsByProject, [projectId]: runs };
  } catch {
    // Silently ignore — project might not exist yet
  }
}

export async function refreshAllLoops(projectIds: string[]): Promise<void> {
  const results = await Promise.allSettled(projectIds.map((id) => loopsApi.list(id)));
  const updated: Record<string, LoopRunSummary[]> = {};
  projectIds.forEach((id, i) => {
    const result = results[i];
    if (result.status === "fulfilled") {
      updated[id] = result.value;
    }
  });
  loopsByProject = { ...loopsByProject, ...updated };
}

// ─── Event listener ──────────────────────────────────────────────────────────

/**
 * Start listening for loop-state-changed events. Call once at app startup.
 * Returns a cleanup function.
 */
export function startLoopEventListener(getProjectIds: () => string[]): () => void {
  if (unlistenFn) return unlistenFn;

  const unlistenPromise = listen("loop-state-changed", () => {
    const projectIds = getProjectIds();
    refreshAllLoops(projectIds);
  });

  const cleanup = () => {
    unlistenPromise.then((fn) => fn());
    unlistenFn = null;
  };
  unlistenFn = cleanup;
  return cleanup;
}
