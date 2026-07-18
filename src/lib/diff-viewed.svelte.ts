/**
 * Per-session in-memory viewed-file state.
 * Tracks which files have been marked as "viewed" in the diff panel.
 * Lives at module level so state persists across component remounts.
 */

interface SessionViewedState {
  viewed: Set<string>;
  fingerprints: Map<string, string>;
  version: number;
}

// Module-level reactive state — survives component remounts
let sessions = $state<Map<string, SessionViewedState>>(new Map());

/**
 * Initialize a session's viewed state if it doesn't exist.
 * Call this from an $effect or onMount — not from inside $derived.
 */
export function ensureSession(sessionId: string): void {
  if (!sessions.has(sessionId)) {
    sessions.set(sessionId, { viewed: new Set(), fingerprints: new Map(), version: 0 });
    sessions = new Map(sessions);
  }
}

function getState(sessionId: string): SessionViewedState {
  return sessions.get(sessionId) ?? { viewed: new Set(), fingerprints: new Map(), version: 0 };
}

function getOrCreate(sessionId: string): SessionViewedState {
  let s = sessions.get(sessionId);
  if (!s) {
    s = { viewed: new Set(), fingerprints: new Map(), version: 0 };
    sessions.set(sessionId, s);
    sessions = new Map(sessions);
  }
  return s;
}

function bump(sessionId: string): void {
  const s = sessions.get(sessionId);
  if (s) {
    s.version++;
    sessions = new Map(sessions);
  }
}

/**
 * Get the viewed files set for a session.
 * Pure read — returns empty set if session is not initialized.
 */
export function getViewedFiles(sessionId: string): Set<string> {
  return getState(sessionId).viewed;
}

/**
 * Mark a file as viewed.
 */
export function setFileViewed(sessionId: string, path: string): void {
  const s = getOrCreate(sessionId);
  s.viewed.add(path);
  s.viewed = new Set(s.viewed);
  bump(sessionId);
}

/**
 * Unmark a file as viewed.
 */
export function setFileUnviewed(sessionId: string, path: string): void {
  const s = getOrCreate(sessionId);
  s.viewed.delete(path);
  s.viewed = new Set(s.viewed);
  bump(sessionId);
}

/**
 * Check if a file is marked as viewed.
 * Pure read — no side effects.
 */
export function isFileViewed(sessionId: string, path: string): boolean {
  return getState(sessionId).viewed.has(path);
}

/**
 * Invalidate viewed marks for files whose fingerprint changed.
 * Compares new fingerprints against internally stored ones from the previous load.
 * Updates the stored fingerprints to the new values.
 * Only triggers reactivity when viewed marks are actually removed.
 */
export function invalidateViewedFiles(
  sessionId: string,
  currentFingerprints: Map<string, string>,
): void {
  const s = getOrCreate(sessionId);
  let changed = false;

  for (const [path, fp] of currentFingerprints) {
    if (s.viewed.has(path) && s.fingerprints.has(path) && s.fingerprints.get(path) !== fp) {
      s.viewed.delete(path);
      changed = true;
    }
  }

  // Store the current fingerprints for future comparisons (internal bookkeeping)
  s.fingerprints = new Map(currentFingerprints);

  if (changed) {
    s.viewed = new Set(s.viewed);
    bump(sessionId);
  }
}

/**
 * Clear all viewed marks for a session.
 */
export function clearViewedFiles(sessionId: string): void {
  const s = sessions.get(sessionId);
  if (s) {
    s.viewed = new Set();
    s.fingerprints = new Map();
    bump(sessionId);
  }
}

/**
 * Remove a session's state entirely. Call when a session is closed
 * to avoid unbounded memory growth.
 */
export function destroySession(sessionId: string): void {
  if (sessions.has(sessionId)) {
    sessions.delete(sessionId);
    sessions = new Map(sessions);
  }
}

/**
 * Get the version counter for a session (increments on any change).
 * Useful for reactive consumers that need to trigger re-renders.
 * Pure read — no side effects.
 */
export function getViewedVersion(sessionId: string): number {
  return getState(sessionId).version;
}
