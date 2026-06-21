/**
 * Session navigation cycle — holds preview state for mod+{/} sidebar cycling.
 * Separate from tab-switcher (Ctrl+Tab MRU overlay) to avoid coupling.
 */

let cycling = $state(false);
let orderedIds = $state<string[]>([]);
let index = $state(0);
let originId = $state<string | null>(null);

export function getPreviewId(): string | null {
  return cycling ? orderedIds[index] ?? null : null;
}

export function isCycling(): boolean {
  return cycling;
}

export function startPreview(ids: string[], currentId: string | undefined, direction: 1 | -1): void {
  if (ids.length <= 1) return;
  orderedIds = ids;
  originId = currentId ?? null;
  const currentIdx = currentId ? ids.indexOf(currentId) : -1;
  index = currentIdx === -1 ? 0 : (currentIdx + direction + ids.length) % ids.length;
  cycling = true;
}

export function advance(direction: 1 | -1): void {
  if (!cycling) return;
  index = (index + direction + orderedIds.length) % orderedIds.length;
}

export function commit(): string | null {
  const target = orderedIds[index] ?? null;
  reset();
  return target;
}

export function cancel(): string | null {
  const origin = originId;
  reset();
  return origin;
}

function reset(): void {
  cycling = false;
  orderedIds = [];
  index = 0;
  originId = null;
}
