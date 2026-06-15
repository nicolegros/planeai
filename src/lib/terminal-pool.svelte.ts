/**
 * Terminal pool — manages which sessions have mounted xterm instances.
 * Only the active session + MRU neighbors are mounted (max 3).
 * Mounted but non-active sessions are "paused" (PTY flow paused).
 */

/** Max simultaneously mounted terminals (active + neighbors) */
export const MAX_MOUNTED = 3;

let mruOrder = $state<string[]>([]);
let active = $state<string | null>(null);

/** Activate a session — promotes it to front of MRU, recomputes mount set. */
export function activateSession(sessionId: string): void {
  active = sessionId;
  mruOrder = [sessionId, ...mruOrder.filter((id) => id !== sessionId)];
}

/** Remove a session from pool tracking entirely. */
export function removeSession(sessionId: string): void {
  mruOrder = mruOrder.filter((id) => id !== sessionId);
  if (active === sessionId) active = mruOrder[0] ?? null;
}

/** Get current pool state (reactive snapshot). */
export function getPoolState(): {
  active: string | null;
  mounted: string[];
  paused: string[];
} {
  const mounted = mruOrder.slice(0, MAX_MOUNTED);
  const paused = mounted.filter((id) => id !== active);
  return { active, mounted, paused };
}

/** Check if a specific session should be mounted. */
export function isMounted(sessionId: string): boolean {
  return mruOrder.indexOf(sessionId) < MAX_MOUNTED && mruOrder.indexOf(sessionId) !== -1;
}

/** Check if a specific session should be paused (mounted but not active). */
export function isPaused(sessionId: string): boolean {
  return isMounted(sessionId) && sessionId !== active;
}
