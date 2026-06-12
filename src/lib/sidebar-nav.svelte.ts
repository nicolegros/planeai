/**
 * Sidebar keyboard navigation state and actions.
 * Manages selectedIndex and translates key presses into named actions.
 */

export type SidebarNavAction =
  | { type: "select" }
  | { type: "archive" }
  | { type: "delete" }
  | { type: "rename" }
  | { type: "restart" }
  | { type: "edit" }
  | { type: "status"; status: string }
  | { type: "start_session" };

let selectedIndex = $state(0);
let pendingKey = $state<string | null>(null);
let pendingTimeout = $state<ReturnType<typeof setTimeout> | null>(null);

export function getSelectedIndex(): number {
  return selectedIndex;
}

export function setSelectedIndex(idx: number): void {
  selectedIndex = idx;
}

export function clampIndex(length: number): void {
  if (selectedIndex >= length) selectedIndex = Math.max(0, length - 1);
}

/**
 * Handle a keydown event for sidebar navigation.
 * Returns a SidebarNavAction if matched, or null if consumed as navigation / no match.
 * The caller should check if zone === "sidebar" before calling.
 */
export function handleSidebarKey(e: KeyboardEvent, listLength: number): SidebarNavAction | null {
  if (listLength === 0) return null;

  const key = e.key;

  // Arrow / j/k navigation (always takes priority)
  if (key === "ArrowDown" || key === "j") {
    e.preventDefault();
    clearPending();
    selectedIndex = Math.min(selectedIndex + 1, listLength - 1);
    return null;
  }
  if (key === "ArrowUp" || key === "k") {
    e.preventDefault();
    clearPending();
    selectedIndex = Math.max(selectedIndex - 1, 0);
    return null;
  }

  // Enter — select
  if (key === "Enter") {
    e.preventDefault();
    clearPending();
    return { type: "select" };
  }

  // Handle pending multi-key sequences first
  if (pendingKey === "d") {
    e.preventDefault();
    clearPending();
    if (key === "d") return { type: "delete" };
    return null;
  }

  if (pendingKey === "s") {
    e.preventDefault();
    clearPending();
    if (key === "t") return { type: "status", status: "todo" };
    if (key === "p") return { type: "status", status: "in_progress" };
    if (key === "r") return { type: "status", status: "in_review" };
    if (key === "d") return { type: "status", status: "done" };
    if (key === "s") return { type: "start_session" };
    return null;
  }

  // Single-key actions (only when no pending)
  if (key === "a") {
    e.preventDefault();
    return { type: "archive" };
  }

  if (key === "r") {
    e.preventDefault();
    return { type: "rename" };
  }

  if (key === "e") {
    e.preventDefault();
    return { type: "edit" };
  }

  if (key === "R") {
    e.preventDefault();
    return { type: "restart" };
  }

  // Start multi-key sequences
  if (key === "d") {
    e.preventDefault();
    setPending("d");
    return null;
  }

  if (key === "s") {
    e.preventDefault();
    setPending("s");
    return null;
  }

  return null;
}

function setPending(key: string) {
  clearPending();
  pendingKey = key;
  pendingTimeout = setTimeout(() => { pendingKey = null; }, 500);
}

function clearPending() {
  if (pendingTimeout) clearTimeout(pendingTimeout);
  pendingTimeout = null;
  pendingKey = null;
}
