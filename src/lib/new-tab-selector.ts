import type { KeyboardAction } from "./keyboard";

export type SelectorChordOutcome = "keep" | "dismiss" | "dismiss-only";

/**
 * What a chord does to the open New tab selector. Every other chord dismisses it, so it never sits
 * under another overlay or keeps a stale pane; closing chords only dismiss it, like closing a sheet.
 */
export function selectorChordOutcome(action: KeyboardAction["type"]): SelectorChordOutcome {
  if (action === "new_tab") return "keep";
  if (action === "close_tab" || action === "close_split") return "dismiss-only";
  return "dismiss";
}
