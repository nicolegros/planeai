import { IS_MAC, isPlatformMod } from "./keyboard";
import type { PluginInventory, PluginUiContribution } from "./types";

export type PluginShortcutTarget = {
  plugin: PluginInventory;
  contribution: PluginUiContribution;
};

function shortcutFor(event: KeyboardEvent): string | null {
  if (!isPlatformMod(event) || (IS_MAC ? event.ctrlKey : event.metaKey)) return null;
  const key = /^Key[A-Z]$/.test(event.code) ? event.code.slice(3) : event.key.toUpperCase();
  if (!/^[A-Z]$/.test(key)) return null;
  return ["Mod", ...(event.shiftKey ? ["Shift"] : []), ...(event.altKey ? ["Alt"] : []), key].join(
    "+",
  );
}

/**
 * Resolve a declared shortcut in the context in which it can be opened.
 * Session panels win because they are scoped to the selected session; main panes
 * remain a fallback for plugin commands that are available more broadly.
 */
export function findPluginShortcut(
  event: KeyboardEvent,
  sessionPanels: PluginShortcutTarget[],
  mainPanes: PluginShortcutTarget[],
): PluginShortcutTarget | null {
  const shortcut = shortcutFor(event);
  if (!shortcut) return null;
  return (
    sessionPanels.find(({ contribution }) => contribution.shortcut === shortcut) ??
    mainPanes.find(({ contribution }) => contribution.shortcut === shortcut) ??
    null
  );
}

/**
 * Mod chords a text field handles itself: caret and selection moves, deletion, clipboard,
 * select all, undo and redo. Plugin frames inline this function's source, so it must not
 * reference anything outside itself.
 */
export function isTextEditingChord(
  event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">,
): boolean {
  if (!(event.metaKey || event.ctrlKey) || event.altKey) return false;
  return (
    /^(ArrowUp|ArrowDown|ArrowLeft|ArrowRight|Backspace|Delete)$/.test(event.key) ||
    /^[acvxyz]$/i.test(event.key)
  );
}
