import { focusTerminal, toggleSidebar } from "./focus.svelte";

export type KeyboardAction =
  | { type: "toggle_sidebar" }
  | { type: "new_session" }
  | { type: "jump_to_session"; index: number }
  | { type: "tab_switch" }
  | { type: "tab_switch_reverse" }
  | { type: "focus_terminal" };

/**
 * Attempt to match a keyboard event to an app-level action.
 * Returns the action if matched, or null if the event should pass through.
 */
export function matchChord(e: KeyboardEvent): KeyboardAction | null {
  const meta = e.metaKey;

  // Escape — always return to terminal
  if (e.key === "Escape") {
    return { type: "focus_terminal" };
  }

  // Ctrl+Tab / Ctrl+Shift+Tab — tab switching
  if (e.ctrlKey && e.key === "Tab") {
    return e.shiftKey ? { type: "tab_switch_reverse" } : { type: "tab_switch" };
  }

  // Cmd/Ctrl+B — toggle sidebar
  if (meta && e.key === "b") {
    return { type: "toggle_sidebar" };
  }

  // Cmd/Ctrl+N — new session
  if (meta && e.key === "n") {
    return { type: "new_session" };
  }

  // Cmd/Ctrl+1-9 — jump to session
  if (meta && e.key >= "1" && e.key <= "9") {
    return { type: "jump_to_session", index: parseInt(e.key) - 1 };
  }

  return null;
}

export type ActionHandler = (action: KeyboardAction) => void;

/**
 * Install the top-level keyboard router on the window.
 * Returns a cleanup function to remove the listener.
 */
export function installKeyboardRouter(onAction: ActionHandler): () => void {
  function handler(e: KeyboardEvent) {
    const action = matchChord(e);
    if (action) {
      e.preventDefault();
      e.stopPropagation();

      // Built-in focus actions
      if (action.type === "focus_terminal") {
        focusTerminal();
      } else if (action.type === "toggle_sidebar") {
        toggleSidebar();
      }

      onAction(action);
    }
  }

  window.addEventListener("keydown", handler, true);
  return () => window.removeEventListener("keydown", handler, true);
}
