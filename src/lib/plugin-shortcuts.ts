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
  return ["Mod", ...(event.shiftKey ? ["Shift"] : []), ...(event.altKey ? ["Alt"] : []), key].join("+");
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
  return sessionPanels.find(({ contribution }) => contribution.shortcut === shortcut)
    ?? mainPanes.find(({ contribution }) => contribution.shortcut === shortcut)
    ?? null;
}
