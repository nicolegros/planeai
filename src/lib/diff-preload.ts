/**
 * Preload cache for diff patches. Populated when agent finishes,
 * consumed by ReviewTab on mount for instant initial load.
 */
import { git } from "./api";
import type { ChangedFile } from "./types";

const patchCache = new Map<string, Map<string, string>>();

/** Preload all file patches for a session's worktree. Single IPC call. */
export function preloadPatches(sessionId: string, repoPath: string, baseBranch: string, files: ChangedFile[]): void {
  const sessionCache = new Map<string, string>();
  patchCache.set(sessionId, sessionCache);
  const fileArgs: [string, string | null][] = files.map((f) => [f.path, f.old_path ?? null]);
  git.getAllFilePatches(repoPath, baseBranch, fileArgs)
    .then((patches) => {
      for (let i = 0; i < files.length; i++) {
        if (patches[i]) sessionCache.set(files[i].path, patches[i]);
      }
    })
    .catch(() => {});
}

/** Get preloaded patches for a session (returns null if not preloaded). */
export function getPreloadedPatches(sessionId: string): Map<string, string> | null {
  return patchCache.get(sessionId) ?? null;
}

/** Clear preloaded patches for a session. */
export function clearPreloadedPatches(sessionId: string): void {
  patchCache.delete(sessionId);
}
