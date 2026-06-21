import { getMruList } from "./mru.svelte";

const SHOW_DELAY_MS = 150;

export type CycleMode = "overlay" | "sidebar";

let isCycling = $state(false);
let isVisible = $state(false);
let cycleList = $state<string[]>([]);
let index = $state(0);
let originSessionId = $state<string | null>(null);
let mode = $state<CycleMode>("overlay");
let showTimer: ReturnType<typeof setTimeout> | null = null;

export function getCycleState() {
  return { isCycling, isVisible, cycleList, index, mode };
}

/** Begin a cycle (MRU overlay). Returns false if nothing to switch to. */
export function startCycle(currentSessionId: string | undefined, validIds?: Set<string>): boolean {
  const mru = getMruList();
  const filtered = validIds ? mru.filter((id) => validIds.has(id)) : mru;
  const others = filtered.filter((id) => id !== currentSessionId);
  if (others.length === 0) return false;

  const current = currentSessionId && filtered.includes(currentSessionId) ? currentSessionId : null;
  cycleList = current ? [...others, current] : [...others];
  originSessionId = currentSessionId ?? null;
  index = 0;
  isCycling = true;
  mode = "overlay";

  showTimer = setTimeout(() => {
    if (isCycling) isVisible = true;
  }, SHOW_DELAY_MS);

  return true;
}

/** Begin a cycle in sidebar order. */
export function startCycleOrdered(orderedIds: string[], currentSessionId: string | undefined, direction: 1 | -1): boolean {
  if (orderedIds.length <= 1) return false;
  cycleList = orderedIds;
  originSessionId = currentSessionId ?? null;
  const currentIdx = currentSessionId ? orderedIds.indexOf(currentSessionId) : -1;
  index = currentIdx === -1 ? 0 : (currentIdx + direction + orderedIds.length) % orderedIds.length;
  isCycling = true;
  isVisible = true;
  mode = "sidebar";
  return true;
}

/** Advance selection forward (+1) or backward (-1). */
export function advance(direction: 1 | -1): void {
  if (!isCycling) return;
  const len = cycleList.length;
  index = (index + direction + len) % len;
}

/** Commit the current selection. Returns the target session ID. */
export function commit(): string | null {
  const target = cycleList[index] ?? null;
  reset();
  return target;
}

/** Cancel the cycle. Returns the original session ID to restore. */
export function cancel(): string | null {
  const origin = originSessionId;
  reset();
  return origin;
}

function reset(): void {
  if (showTimer !== null) {
    clearTimeout(showTimer);
    showTimer = null;
  }
  isCycling = false;
  isVisible = false;
  cycleList = [];
  index = 0;
  originSessionId = null;
  mode = "overlay";
}
