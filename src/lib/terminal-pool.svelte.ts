/**
 * Terminal pool — manages which sessions have mounted xterm instances.
 * Only the active session + MRU neighbors are mounted (max 3 by default).
 * Mounted but non-active sessions are "paused" (PTY flow paused).
 *
 * Derives mount decisions from the global MRU list (mru.svelte.ts).
 */
import { getMruList, touchMru, removeMru } from "./mru.svelte";
import { getSettings } from "./settings.svelte";

/** Default max simultaneously mounted terminals (active + neighbors) */
export const DEFAULT_MAX_MOUNTED = 3;

/** @deprecated Use getSettings().max_mounted_terminals ?? DEFAULT_MAX_MOUNTED */
export { DEFAULT_MAX_MOUNTED as MAX_MOUNTED };

function getMaxMounted(): number {
  return getSettings().max_mounted_terminals ?? DEFAULT_MAX_MOUNTED;
}

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
  const maxMounted = getMaxMounted();
  const mounted = getMruList().slice(0, maxMounted);
  const paused = mounted.filter((id) => id !== active);
  return { active, mounted, paused };
}

/** Check if a specific session should be mounted. */
export function isMounted(sessionId: string): boolean {
  const mru = getMruList();
  const idx = mru.indexOf(sessionId);
  return idx !== -1 && idx < getMaxMounted();
}

/** Check if a specific session should be paused (mounted but not active). */
export function isPaused(sessionId: string): boolean {
  return isMounted(sessionId) && sessionId !== active;
}
