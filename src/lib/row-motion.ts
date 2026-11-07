/** How sidebar rows move: a moved row glides to its new list, a row without a counterpart fades and slides in or out, and nothing moves under reduced motion. */
import { cubicOut } from "svelte/easing";
import { crossfade } from "svelte/transition";

export const reducedMotion = (): boolean =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const motionMs = () => (reducedMotion() ? 0 : 150);

/** For a row's inline `transition` as it glides home; cubic-bezier(0.33, 1, 0.68, 1) is cubicOut. */
export const rowGlide = (): string =>
  reducedMotion() ? "" : "transform 150ms cubic-bezier(0.33, 1, 0.68, 1)";

/** For `animate:flip` on a row. */
export const rowFlip = { duration: motionMs, easing: cubicOut };

/** For `out:sendRow` and `in:receiveRow` on a row, keyed by its nav key. */
export const [sendRow, receiveRow] = crossfade({
  duration: motionMs,
  easing: cubicOut,
  fallback: () => ({
    duration: motionMs(),
    easing: cubicOut,
    css: (t) => `opacity: ${t}; transform: translateY(${(t - 1) * 8}px)`,
  }),
});
