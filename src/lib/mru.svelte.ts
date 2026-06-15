/**
 * MRU (Most Recently Used) session tracker.
 * Maintains an ordered list of session IDs, most recent first.
 */
import { sessions } from "./api";

let mruList = $state<string[]>([]);

export function getMruList(): string[] {
  return mruList;
}

/** Push a session to the front of the MRU list. */
export function touchMru(sessionId: string): void {
  mruList = [sessionId, ...mruList.filter((id) => id !== sessionId)];
  sessions.saveMruOrder(mruList).catch(() => {});
}

/** Remove a session from the MRU list. */
export function removeMru(sessionId: string): void {
  mruList = mruList.filter((id) => id !== sessionId);
}

/** Initialize MRU list from persisted order (no persistence triggered). */
export function seedMru(sessionIds: string[]): void {
  mruList = sessionIds;
}

/** Flush MRU save (no-op, persists on every switch). */
export async function flushMru(): Promise<void> {
  await sessions.saveMruOrder(mruList);
}
