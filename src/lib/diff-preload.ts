/**
 * Preload cache for diff patches. Populated when agent finishes,
 * consumed by ReviewTab on mount for instant initial load.
 */
import { git } from "./api";
import type { ChangedFile } from "./types";

const patchCache = new Map<string, Map<string, string>>();

/** Preload all file patches for a session's worktree. */
export function preloadPatches(sessionId: string, repoPath: string, baseBranch: string, files: ChangedFile[]): void {
  const sessionCache = new Map<string, string>();
  patchCache.set(sessionId, sessionCache);
  // Fetch all patches in parallel (non-blocking)
  for (const file of files) {
    git.getFilePatch(repoPath, baseBranch, file.path, file.old_path ?? null)
      .then((patch) => { if (patch) sessionCache.set(file.path, patch); })
      .catch(() => {});
  }
}

/** Get preloaded patches for a session (returns null if not preloaded). */
export function getPreloadedPatches(sessionId: string): Map<string, string> | null {
  return patchCache.get(sessionId) ?? null;
}

/** Clear preloaded patches for a session. */
export function clearPreloadedPatches(sessionId: string): void {
  patchCache.delete(sessionId);
}
