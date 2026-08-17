/**
 * Preload cache for diff patches. Populated when agent finishes,
 * consumed by ReviewTab on mount for instant initial load.
 */
import { git } from "./api";

interface PreloadedPatch {
  patch: string;
  baseBranch: string;
  headRef: string | null;
}

const patchCache = new Map<string, PreloadedPatch>();
const cacheVersions = new Map<string, number>();
let nextCacheVersion = 0;

function invalidatePreloadedPatches(sessionId: string): void {
  patchCache.delete(sessionId);
  cacheVersions.set(sessionId, ++nextCacheVersion);
}

/** Preload the combined patch for a session's worktree. Single IPC call. */
export function preloadPatches(
  sessionId: string,
  repoPath: string,
  baseBranch: string,
  headRef: string | null = null,
): void {
  patchCache.delete(sessionId);
  const cacheVersion = ++nextCacheVersion;
  cacheVersions.set(sessionId, cacheVersion);
  git
    .getCombinedPatch(repoPath, baseBranch, headRef)
    .then((patch) => {
      if (cacheVersions.get(sessionId) === cacheVersion) {
        patchCache.set(sessionId, { patch, baseBranch, headRef });
      }
    })
    .catch(() => {});
}

function getMatchingPreloadedPatch(
  sessionId: string,
  baseBranch: string,
  headRef: string | null,
): string | null {
  const preloaded = patchCache.get(sessionId);
  return preloaded?.baseBranch === baseBranch && preloaded.headRef === headRef
    ? preloaded.patch
    : null;
}

/** Clear preloaded patches for a session. */
export function clearPreloadedPatches(sessionId: string): void {
  invalidatePreloadedPatches(sessionId);
}

/** Release preload state when a session is permanently removed. */
export function disposePreloadedPatches(sessionId: string): void {
  invalidatePreloadedPatches(sessionId);
  cacheVersions.delete(sessionId);
}

/**
 * Load the patch displayed in a review tab. Only the initial load may use the
 * Idle-time preload; manual reloads must read the current working tree.
 */
export async function getCombinedPatchForReview(
  sessionId: string,
  repoPath: string,
  baseBranch: string,
  headRef: string | null,
  usePreloaded = false,
): Promise<string> {
  const preloaded = usePreloaded ? getMatchingPreloadedPatch(sessionId, baseBranch, headRef) : null;
  clearPreloadedPatches(sessionId);
  return preloaded ?? git.getCombinedPatch(repoPath, baseBranch, headRef);
}
