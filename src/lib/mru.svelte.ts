/**
 * MRU (Most Recently Used) session tracker + active session pool.
 * Maintains an ordered list of session IDs, most recent first.
 * All sessions in the MRU list are considered "mounted" (kept alive).
 */
import { sessions } from "./api";

let mruList = $state<string[]>([]);
let active = $state<string | null>(null);
let saveTimer: ReturnType<typeof setTimeout> | null = null;
const SAVE_DEBOUNCE_MS = 500;

function scheduleSave(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    sessions.saveMruOrder(mruList).catch(() => {});
  }, SAVE_DEBOUNCE_MS);
}

export function getMruList(): string[] {
  return mruList;
}

/** Push a session to the front of the MRU list. */
export function touchMru(sessionId: string): void {
  mruList = [sessionId, ...mruList.filter((id) => id !== sessionId)];
  scheduleSave();
}

/** Remove a session from the MRU list. */
export function removeMru(sessionId: string): void {
  mruList = mruList.filter((id) => id !== sessionId);
  scheduleSave();
}

/** Initialize MRU list from persisted order (no persistence triggered). */
export function seedMru(sessionIds: string[]): void {
  mruList = sessionIds;
}

/** Flush MRU save immediately (e.g. before quit). */
export async function flushMru(): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  await sessions.saveMruOrder(mruList);
}

// ─── Pool (active session tracking) ─────────────────────────────────────────

/** Activate a session — promotes it in MRU and sets it as active. */
export function activateSession(sessionId: string): void {
  active = sessionId;
  touchMru(sessionId);
}

/** Remove a session from pool tracking entirely. */
export function removeSession(sessionId: string): void {
  removeMru(sessionId);
  if (active === sessionId) active = mruList[0] ?? null;
}

/** Check if a specific session is in the MRU (always true for tracked sessions). */
export function isMounted(sessionId: string): boolean {
  return mruList.includes(sessionId);
}
