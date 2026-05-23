/**
 * MRU (Most Recently Used) session tracker.
 * Maintains an ordered list of session IDs, most recent first.
 */
let mruList = $state<string[]>([]);

export function getMruList(): string[] {
  return mruList;
}

/** Push a session to the front of the MRU list. */
export function touchMru(sessionId: string): void {
  mruList = [sessionId, ...mruList.filter((id) => id !== sessionId)];
}

/** Remove a session from the MRU list. */
export function removeMru(sessionId: string): void {
  mruList = mruList.filter((id) => id !== sessionId);
}
