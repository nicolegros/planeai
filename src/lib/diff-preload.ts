/**
 * Preload cache for diff patches. Populated when agent finishes,
 * consumed by ReviewTab on mount for instant initial load.
 */
import { git } from "./api";

const patchCache = new Map<string, string>();

/** Preload the combined patch for a session's worktree. Single IPC call. */
export function preloadPatches(sessionId: string, repoPath: string, baseBranch: string): void {
  git
    .getCombinedPatch(repoPath, baseBranch)
    .then((patch) => {
      patchCache.set(sessionId, patch);
    })
    .catch(() => {});
}

/** Get preloaded combined patch for a session (returns null if not preloaded). */
export function getPreloadedPatches(sessionId: string): string | null {
  return patchCache.get(sessionId) ?? null;
}

/** Clear preloaded patches for a session. */
export function clearPreloadedPatches(sessionId: string): void {
  patchCache.delete(sessionId);
}
