/**
 * Loop store — reactive state for loop runs, with event-driven refresh.
 *
 * Listens for `loop-state-changed` events from the backend and refreshes.
 * Also supports manual refresh for the active loop detail view.
 */

import { listen } from "@tauri-apps/api/event";
import { loops as loopsApi } from "./api";
import type { LoopRunSummary, LoopSessionItem } from "./types";

// ─── State ───────────────────────────────────────────────────────────────────

let loopsByProject = $state<Record<string, LoopRunSummary[]>>({});
let activeLoopId = $state<string | null>(null);
/** Maps session ID → loop ID for all known loop sessions */
let sessionToLoop = $state<Record<string, string>>({});
/** Maps loop ID → its session items (role, round, etc.) */
let loopSessions = $state<Record<string, LoopSessionItem[]>>({});
let unlistenFn: (() => void) | null = null;

// ─── Getters ─────────────────────────────────────────────────────────────────

export function getLoopsForProject(projectId: string): LoopRunSummary[] {
  return loopsByProject[projectId] ?? [];
}

export function getActiveLoopId(): string | null {
  return activeLoopId;
}

/** Returns the loop ID a session belongs to, or null if it's not a loop session */
export function getLoopIdForSession(sessionId: string): string | null {
  return sessionToLoop[sessionId] ?? null;
}

/** Returns the session items for a given loop */
export function getSessionsForLoop(loopId: string): LoopSessionItem[] {
  return loopSessions[loopId] ?? [];
}

// ─── Actions ─────────────────────────────────────────────────────────────────

export function setActiveLoopId(id: string | null) {
  activeLoopId = id;
}

export async function refreshLoopsForProject(projectId: string): Promise<void> {
  try {
    const runs = await loopsApi.list(projectId);
    loopsByProject = { ...loopsByProject, [projectId]: runs };
    await refreshLoopSessions(runs);
  } catch {
    // Silently ignore — project might not exist yet
  }
}

export async function refreshAllLoops(projectIds: string[]): Promise<void> {
  const results = await Promise.allSettled(projectIds.map((id) => loopsApi.list(id)));
  const updated: Record<string, LoopRunSummary[]> = {};
  const allRuns: LoopRunSummary[] = [];
  projectIds.forEach((id, i) => {
    const result = results[i];
    if (result.status === "fulfilled") {
      updated[id] = result.value;
      allRuns.push(...result.value);
    }
  });
  loopsByProject = { ...loopsByProject, ...updated };
  await refreshLoopSessions(allRuns);
}

/** Fetch detail for loops that may have sessions and update mappings */
async function refreshLoopSessions(runs: LoopRunSummary[]): Promise<void> {
  // Only fetch detail for loops that are not draft (drafts have no sessions)
  const active = runs.filter((r) => r.status !== "draft");
  if (active.length === 0) return;

  const details = await Promise.allSettled(active.map((r) => loopsApi.detail(r.id)));
  const newMapping: Record<string, string> = { ...sessionToLoop };
  const newSessions: Record<string, LoopSessionItem[]> = { ...loopSessions };

  details.forEach((result, i) => {
    if (result.status !== "fulfilled") return;
    const detail = result.value;
    newSessions[detail.run.id] = detail.sessions;
    for (const s of detail.sessions) {
      newMapping[s.session_id] = detail.run.id;
    }
  });

  sessionToLoop = newMapping;
  loopSessions = newSessions;
}

// ─── Event listener ──────────────────────────────────────────────────────────

/**
 * Start listening for loop-state-changed events. Call once at app startup.
 * Returns a cleanup function.
 */
export function startLoopEventListener(getProjectIds: () => string[]): () => void {
  if (unlistenFn) return unlistenFn;

  const refresh = () => {
    const projectIds = getProjectIds();
    refreshAllLoops(projectIds);
  };

  // Debounced refresh for high-frequency events (agent state changes, session changes)
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  const debouncedRefresh = () => {
    if (debounceTimer) return; // already scheduled
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      refresh();
    }, 2000);
  };

  const unlistenLoop = listen("loop-state-changed", refresh);
  // Also refresh on sessions-changed and agent-state-change because
  // auto_advance (recipe tick via agent handoff) doesn't emit loop-state-changed.
  const unlistenSessions = listen("sessions-changed", debouncedRefresh);
  const unlistenAgent = listen("agent-state-change", debouncedRefresh);

  const cleanup = () => {
    if (debounceTimer) clearTimeout(debounceTimer);
    unlistenLoop.then((fn) => fn());
    unlistenSessions.then((fn) => fn());
    unlistenAgent.then((fn) => fn());
    unlistenFn = null;
  };
  unlistenFn = cleanup;
  return cleanup;
}
