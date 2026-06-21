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
  // Fetch patches with limited concurrency to avoid flooding IPC
  let i = 0;
  const next = () => {
    if (i >= files.length) return;
    const file = files[i++];
    git.getFilePatch(repoPath, baseBranch, file.path, file.old_path ?? null)
      .then((patch) => { if (patch) sessionCache.set(file.path, patch); })
      .catch(() => {})
      .finally(next);
  };
  // Start 3 concurrent fetches
  for (let j = 0; j < Math.min(3, files.length); j++) next();
}

/** Get preloaded patches for a session (returns null if not preloaded). */
export function getPreloadedPatches(sessionId: string): Map<string, string> | null {
  return patchCache.get(sessionId) ?? null;
}

/** Clear preloaded patches for a session. */
export function clearPreloadedPatches(sessionId: string): void {
  patchCache.delete(sessionId);
}
