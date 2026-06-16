/**
 * Terminal pool — manages which sessions have mounted xterm instances.
 * Only the active session + MRU neighbors are mounted (max 3).
 * Mounted but non-active sessions are "paused" (PTY flow paused).
 *
 * Derives mount decisions from the global MRU list (mru.svelte.ts).
 */
import { getMruList, touchMru, removeMru } from "./mru.svelte";

/** Max simultaneously mounted terminals (active + neighbors) */
export const MAX_MOUNTED = 3;

let active = $state<string | null>(null);

/** Activate a session — promotes it in MRU and sets it as active. */
export function activateSession(sessionId: string): void {
  active = sessionId;
  touchMru(sessionId);
}

/** Remove a session from pool tracking entirely. */
export function removeSession(sessionId: string): void {
  removeMru(sessionId);
  if (active === sessionId) active = getMruList()[0] ?? null;
}

/** Get current pool state (reactive snapshot). */
export function getPoolState(): {
  active: string | null;
  mounted: string[];
  paused: string[];
} {
  const mounted = getMruList().slice(0, MAX_MOUNTED);
  const paused = mounted.filter((id) => id !== active);
  return { active, mounted, paused };
}

/** Check if a specific session should be mounted. */
export function isMounted(sessionId: string): boolean {
  const mru = getMruList();
  const idx = mru.indexOf(sessionId);
  return idx !== -1 && idx < MAX_MOUNTED;
}

/** Check if a specific session should be paused (mounted but not active). */
export function isPaused(sessionId: string): boolean {
  return isMounted(sessionId) && sessionId !== active;
}
