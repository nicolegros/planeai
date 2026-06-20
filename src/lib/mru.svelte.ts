/**
 * MRU (Most Recently Used) session tracker.
 * Maintains an ordered list of session IDs, most recent first.
 */
import { sessions } from "./api";

let mruList = $state<string[]>([]);
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
