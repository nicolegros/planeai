import { IS_MAC } from "./keyboard";

export type TerminalKeyAction =
  | { type: "copy" }
  | { type: "paste" }
  | { type: "send_bytes"; bytes: number[] }
  | { type: "scroll_page_up" }
  | { type: "scroll_page_down" }
  | { type: "scroll_line_up" }
  | { type: "scroll_line_down" }
  | { type: "passthrough" }
  | null;

/**
 * Match a keyboard event in the terminal context to an action.
 * Returns null if the event should be handled by xterm normally.
 * "passthrough" means let the browser/native handle it (e.g. paste).
 */
export function matchTerminalKey(
  e: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; altKey: boolean },
  hasSelection: boolean,
): TerminalKeyAction {
  if (IS_MAC) {
    // Cmd+C with selection → copy
    if (e.metaKey && !e.ctrlKey && !e.shiftKey && e.key === "c") {
      return hasSelection ? { type: "copy" } : null;
    }
    // Cmd+V → paste (passthrough to native)
    if (e.metaKey && !e.ctrlKey && !e.shiftKey && e.key === "v") {
      return { type: "passthrough" };
    }
    // Cmd+Backspace → Ctrl+U (kill line)
    if (e.metaKey && !e.ctrlKey && !e.shiftKey && e.key === "Backspace") {
      return { type: "send_bytes", bytes: [0x15] };
    }
    // Cmd+Left → Ctrl+A (beginning of line)
    if (e.metaKey && !e.ctrlKey && !e.shiftKey && e.key === "ArrowLeft") {
      return { type: "send_bytes", bytes: [0x01] };
    }
    // Cmd+Right → Ctrl+E (end of line)
    if (e.metaKey && !e.ctrlKey && !e.shiftKey && e.key === "ArrowRight") {
      return { type: "send_bytes", bytes: [0x05] };
    }
  } else {
    // Ctrl+Shift+C → copy (always consume to prevent WebView2 opening devtools)
    if (e.ctrlKey && e.shiftKey && !e.metaKey && e.key === "C") {
      return { type: "copy" };
    }
    // Ctrl+C with selection → copy
    if (e.ctrlKey && !e.shiftKey && !e.metaKey && e.key === "c") {
      return hasSelection ? { type: "copy" } : null;
    }
    // Ctrl+Shift+V → paste
    if (e.ctrlKey && e.shiftKey && !e.metaKey && e.key === "V") {
      return { type: "paste" };
    }
    // Ctrl+V → paste (passthrough to native)
    if (e.ctrlKey && !e.shiftKey && !e.metaKey && e.key === "v") {
      return { type: "passthrough" };
    }
    // Home → Ctrl+A (beginning of line)
    if (!e.ctrlKey && !e.metaKey && !e.shiftKey && e.key === "Home") {
      return { type: "send_bytes", bytes: [0x01] };
    }
    // End → Ctrl+E (end of line)
    if (!e.ctrlKey && !e.metaKey && !e.shiftKey && e.key === "End") {
      return { type: "send_bytes", bytes: [0x05] };
    }
    // Ctrl+Backspace → Ctrl+U (kill line)
    if (e.ctrlKey && !e.shiftKey && !e.metaKey && e.key === "Backspace") {
      return { type: "send_bytes", bytes: [0x15] };
    }
  }

  // Shift+PageUp/PageDown → scroll terminal buffer
  if (e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && e.key === "PageUp") {
    return { type: "scroll_page_up" };
  }
  if (e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && e.key === "PageDown") {
    return { type: "scroll_page_down" };
  }
  // Shift+ArrowUp/ArrowDown → scroll terminal buffer line by line
  if (e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && e.key === "ArrowUp") {
    return { type: "scroll_line_up" };
  }
  if (e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && e.key === "ArrowDown") {
    return { type: "scroll_line_down" };
  }

  // Shift+Enter → Ctrl+J (newline without submit)
  if (e.shiftKey && !e.ctrlKey && !e.metaKey && e.key === "Enter") {
    return { type: "send_bytes", bytes: [0x0a] };
  }

  // Escape → Ctrl+C (interrupt)
  if (e.key === "Escape" && !e.ctrlKey && !e.metaKey && !e.altKey) {
    return { type: "send_bytes", bytes: [0x03] };
  }

  return null;
}
