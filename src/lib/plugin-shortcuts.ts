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

/**
 * The key event to replay for a frame's `host-key` message, or `null` when it may not be
 * replayed. The frame's own script can post anything, so only chords and their modifier
 * release pass; the host also requires the frame to have the keyboard.
 */
export function hostKeyReplay(message: Record<string, unknown>): KeyboardEvent | null {
  const key = typeof message.key === "string" ? message.key : "";
  const phase = message.phase === "keydown" || message.phase === "keyup" ? message.phase : null;
  const replayable =
    phase === "keydown"
      ? message.ctrlKey === true || message.metaKey === true
      : phase === "keyup" && (key === "Control" || key === "Meta");
  if (!phase || !replayable) return null;
  return new KeyboardEvent(phase, {
    key,
    code: typeof message.code === "string" ? message.code : "",
    altKey: message.altKey === true,
    ctrlKey: message.ctrlKey === true,
    metaKey: message.metaKey === true,
    shiftKey: message.shiftKey === true,
    repeat: message.repeat === true,
    bubbles: true,
    cancelable: true,
  });
}
