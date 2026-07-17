import { tick } from "svelte";

/**
 * Ensures the first combobox item is highlighted when no item currently has
 * `data-highlighted`. This works around bits-ui not auto-highlighting after
 * external filtering changes the visible item set.
 *
 * Implementation: dispatches a synthetic `pointermove` on the first item,
 * which is how bits-ui@2.18.x triggers its internal highlight state.
 * If bits-ui changes this mechanism, this function will need updating.
 */
export async function ensureFirstItemHighlighted(contentRef: HTMLElement | null): Promise<void> {
  await tick();
  if (!contentRef || !contentRef.isConnected) return;
  const highlighted = contentRef.querySelector("[data-highlighted]");
  if (highlighted) return;
  const first = contentRef.querySelector<HTMLElement>("[data-combobox-item]");
  if (!first) return;
  const rect = first.getBoundingClientRect();
  // Skip if element hasn't been laid out yet (e.g., during CSS transitions)
  if (rect.width === 0 || rect.height === 0) return;
  first.dispatchEvent(
    new PointerEvent("pointermove", {
      bubbles: true,
      cancelable: true,
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
    }),
  );
}

/**
 * Handles Enter key fallback for combobox selection. If no item is highlighted
 * by bits-ui, selects the first filtered item and prevents further event
 * propagation to avoid double-firing with bits-ui's own handler.
 *
 * Returns `true` if the fallback was applied (caller should stop further handling).
 */
export function selectFirstIfNoHighlight(
  e: KeyboardEvent,
  contentRef: HTMLElement | null,
  firstValue: string,
  selectFn: (value: string) => void,
): boolean {
  const highlighted = contentRef?.querySelector("[data-highlighted]");
  if (highlighted) return false;
  e.preventDefault();
  e.stopImmediatePropagation();
  selectFn(firstValue);
  return true;
}
