import type { FocusZone } from "./focus.svelte";

/**
 * Whether the Review tab's window-level shortcuts apply to a keydown: only while its pane
 * owns the keyboard, so a chord replayed from another pane's plugin frame cannot send its
 * feedback, and only for keys nothing else handled.
 */
export function reviewShortcutsApply(state: {
  visible: boolean;
  focused: boolean;
  defaultPrevented: boolean;
  zone: FocusZone;
}): boolean {
  return state.visible && state.focused && !state.defaultPrevented && state.zone === "terminal";
}
