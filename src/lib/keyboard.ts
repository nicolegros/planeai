import {
  focusTerminal,
  getActiveZone,
  toggleSidebar,
  toggleSessionsPanel,
  toggleTaskPanel,
} from "./focus.svelte";

/** True on macOS/iOS, false on Windows/Linux */
export const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/** Returns the platform modifier label: ⌘ on macOS, Ctrl on Windows/Linux */
export const MOD_LABEL = IS_MAC ? "⌘" : "Ctrl+";

/** Hint text for Mod+Enter submit shortcut */
export const MOD_ENTER_HINT = IS_MAC ? "⌘↵" : "Ctrl+↵";

/** Check if the platform modifier key is pressed (Cmd on macOS, Ctrl on Windows/Linux) */
export function isPlatformMod(e: KeyboardEvent): boolean {
  return IS_MAC ? e.metaKey : e.ctrlKey;
}

export type KeyboardAction =
  | { type: "toggle_sidebar" }
  | { type: "new_session" }
  | { type: "new_project" }
  | { type: "jump_to_session"; index: number }
  | { type: "next_session" }
  | { type: "prev_session" }
  | { type: "tab_switch" }
  | { type: "tab_switch_reverse" }
  | { type: "command_palette" }
  | { type: "focus_terminal" }
  | { type: "open_preferences" }
  | { type: "new_tab" }
  | { type: "close_tab" }
  | { type: "next_tab" }
  | { type: "prev_tab" }
  | { type: "toggle_diff" }
  | { type: "toggle_file_explorer" }
  | { type: "toggle_sessions_panel" }
  | { type: "toggle_task_panel" }
  | { type: "refresh_tasks" }
  | { type: "open_file" }
  | { type: "save_file" }
  | { type: "show_shortcuts" };

/**
 * Attempt to match a keyboard event to an app-level action.
 * Returns the action if matched, or null if the event should pass through.
 */
export function matchChord(e: KeyboardEvent): KeyboardAction | null {
  const mod = isPlatformMod(e);
  const key = e.key.toLowerCase();

  // Escape — always return to terminal
  if (e.key === "Escape") {
    return { type: "focus_terminal" };
  }

  // Ctrl+Tab / Ctrl+Shift+Tab — tab switching
  if (e.ctrlKey && e.key === "Tab") {
    return e.shiftKey ? { type: "tab_switch_reverse" } : { type: "tab_switch" };
  }

  // Mod+B — toggle sidebar
  if (mod && key === "b") {
    return { type: "toggle_sidebar" };
  }

  // Mod+K — command palette
  if (mod && key === "k") {
    return { type: "command_palette" };
  }

  // Mod+, — preferences
  if (mod && e.key === ",") {
    return { type: "open_preferences" };
  }

  // Mod+T — new tab
  if (mod && !e.shiftKey && key === "t") {
    return { type: "new_tab" };
  }

  // Mod+Shift+T — toggle task panel
  if (mod && e.shiftKey && key === "t") {
    return { type: "toggle_task_panel" };
  }

  // Mod+W — close tab
  if (mod && !e.shiftKey && key === "w") {
    return { type: "close_tab" };
  }

  // Mod+] — next tab
  if (mod && !e.shiftKey && (e.key === "]")) {
    return { type: "next_tab" };
  }

  // Mod+[ — prev tab
  if (mod && !e.shiftKey && (e.key === "[")) {
    return { type: "prev_tab" };
  }

  // Mod+{ (mod+Shift+[) — next session
  if (mod && e.shiftKey && (e.key === "}" || e.key === "]")) {
    return { type: "next_session" };
  }

  // Mod+} (mod+Shift+]) — prev session
  if (mod && e.shiftKey && (e.key === "{" || e.key === "[")) {
    return { type: "prev_session" };
  }

  // Mod+D — toggle diff tab
  if (mod && !e.shiftKey && key === "d") {
    return { type: "toggle_diff" };
  }

  // Mod+E — toggle file explorer
  if (mod && !e.shiftKey && key === "e") {
    return { type: "toggle_file_explorer" };
  }

  // Mod+P — open file finder
  if (mod && !e.shiftKey && key === "p") {
    return { type: "open_file" };
  }

  // Mod+Shift+S — toggle sessions panel
  if (mod && e.shiftKey && key === "s") {
    return { type: "toggle_sessions_panel" };
  }

  // Mod+S — save file
  if (mod && !e.shiftKey && key === "s") {
    return { type: "save_file" };
  }

  // Mod+R — refresh tasks
  if (mod && !e.shiftKey && key === "r") {
    return { type: "refresh_tasks" };
  }

  // Mod+Shift+R — review changes (toggle diff/review tab)
  if (mod && e.shiftKey && key === "r") {
    return { type: "toggle_diff" };
  }

  // Mod+/ — keyboard shortcuts
  if (mod && !e.shiftKey && e.key === "/") {
    return { type: "show_shortcuts" };
  }

  // Mod+Shift+N — new project
  if (mod && e.shiftKey && key === "n") {
    return { type: "new_project" };
  }

  // Mod+N — new session
  if (mod && key === "n") {
    return { type: "new_session" };
  }

  // Mod+1-9 — jump to session
  if (mod && e.key >= "1" && e.key <= "9") {
    return { type: "jump_to_session", index: parseInt(e.key) - 1 };
  }

  return null;
}

export type ActionHandler = (action: KeyboardAction) => void;

/**
 * Install the top-level keyboard router on the window.
 * Returns a cleanup function to remove the listener.
 */
export function installKeyboardRouter(
  onAction: ActionHandler,
  shouldPassEscape?: () => boolean,
  isEditorFocused?: () => boolean,
): () => void {
  const editorAllowedActions = new Set<KeyboardAction["type"]>([
    "open_file",
    "toggle_file_explorer",
    "toggle_diff",
    "command_palette",
    "tab_switch",
    "tab_switch_reverse",
    "save_file",
    "close_tab",
    "new_tab",
    "next_tab",
    "prev_tab",
    "next_session",
    "prev_session",
    "open_preferences",
  ]);

  function handler(e: KeyboardEvent) {
    const action = matchChord(e);
    if (action) {
      // When editor is focused, only intercept whitelisted actions
      if (isEditorFocused?.() && !editorAllowedActions.has(action.type)) {
        return;
      }

      // If Escape and terminal already focused with no overlays, let it pass through
      if (
        action.type === "focus_terminal" &&
        getActiveZone() === "terminal" &&
        (!shouldPassEscape || shouldPassEscape())
      ) {
        return;
      }

      e.preventDefault();
      e.stopPropagation();

      // Built-in focus actions
      if (action.type === "focus_terminal") {
        focusTerminal();
      } else if (action.type === "toggle_sidebar") {
        toggleSidebar();
      } else if (action.type === "toggle_sessions_panel") {
        toggleSessionsPanel();
      } else if (action.type === "toggle_task_panel") {
        toggleTaskPanel();
      }

      onAction(action);
    }
  }

  window.addEventListener("keydown", handler, true);
  return () => window.removeEventListener("keydown", handler, true);
}
