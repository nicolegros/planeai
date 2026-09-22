/**
 * Workspace resources — opens shells, diffs and embedded editors into the
 * TaskWorkspace layout.
 *
 * Every opener funnels through `splitTree.addSessionToLeaf`, which keys tabs by
 * pty key. Callers own the UI follow-up (focus, snackbars); this module owns the
 * layout and session-tab bookkeeping so the two cannot drift apart.
 */
import { pty } from "./api";
import { addTab, removeTab } from "./session-tabs.svelte";
import * as splitTree from "./split-tree.svelte";
import type { SplitDirection } from "./split-tree.svelte";

/** Outcome of opening a resource that may already be in the layout. */
export type ResourceOpen = "opened" | "focused" | "closed" | "unavailable";

/**
 * Place a shell tab for an already-allocated pty key into a leaf. Shell tabs
 * created outside `openShellResource` (terminal editor tabs, which allocate
 * their pty key ahead of time) go through here so every shell tab looks alike.
 *
 * Returns whether the tab is in the layout afterwards. A caller that resolved
 * `leafId` before an await must treat false as a failure and roll back.
 */
export function addShellTabToLeaf(leafId: string, ptyKey: string, label = "Shell"): boolean {
  return splitTree.addSessionToLeaf(leafId, { ptyKey, label, icon: "terminal", type: "shell" });
}

/**
 * Add a shell tab for a session to an existing leaf. Returns its pty key, or
 * null when the session has no tab state to extend.
 */
export function openShellResource(sessionId: string, leafId: string): string | null {
  const tabIndex = addTab(sessionId);
  if (tabIndex === -1) return null;

  const ptyKey = `${sessionId}:${tabIndex}`;
  // A pty key already in the layout would mean the session tab counter fell out
  // of step with the tree; give the index back rather than reusing the key.
  if (splitTree.getLeafForSession(ptyKey)) {
    removeTab(sessionId, tabIndex);
    return null;
  }

  pty.incrementTabCount(sessionId);
  addShellTabToLeaf(leafId, ptyKey);
  return ptyKey;
}

/**
 * Split the focused pane and open a shell tab in the new pane. The split is
 * undone when the shell tab cannot be created, so no empty pane is left behind.
 */
export function openShellResourceInNewPane(
  sessionId: string,
  direction: SplitDirection,
): string | null {
  const newLeafId = splitTree.splitFocusedLeaf(direction);
  if (!newLeafId) return null;

  const ptyKey = openShellResource(sessionId, newLeafId);
  if (!ptyKey) {
    splitTree.destroyLeaf(newLeafId);
    return null;
  }
  return ptyKey;
}

/** Open a shell tab in the focused leaf. */
export function openShellResourceInFocusedLeaf(sessionId: string): string | null {
  const focusedLeafId = splitTree.getFocusedLeafId();
  if (!focusedLeafId) return null;
  return openShellResource(sessionId, focusedLeafId);
}

/** Pty key for a session's diff resource. */
function diffPtyKey(sessionId: string): string {
  return `${sessionId}:diff`;
}

/** Pty key for a session's embedded editor resource for one file. */
function editorPtyKey(sessionId: string, filePath: string): string {
  return `${sessionId}:editor:${filePath}`;
}

/**
 * Toggle the diff resource: open it in the focused leaf, focus it when it is
 * open elsewhere, or close it when it is already the active tab.
 */
export function toggleDiffResource(sessionId: string): ResourceOpen {
  const ptyKey = diffPtyKey(sessionId);
  const existing = splitTree.findTab(ptyKey);
  if (existing) {
    if (existing.leaf.activeTab === ptyKey) {
      splitTree.removeSessionFromLeaf(ptyKey);
      return "closed";
    }
    splitTree.focusTab(ptyKey);
    return "focused";
  }

  const focusedLeafId = splitTree.getFocusedLeafId();
  if (!focusedLeafId) return "unavailable";
  splitTree.addSessionToLeaf(focusedLeafId, {
    ptyKey,
    label: "Diff",
    icon: "git-compare",
    type: "diff",
  });
  return "opened";
}

/** Open a file in the embedded editor, focusing an already-open tab for it. */
export function openEditorResource(sessionId: string, filePath: string): ResourceOpen {
  const ptyKey = editorPtyKey(sessionId, filePath);
  if (splitTree.focusTab(ptyKey)) return "focused";

  const focusedLeafId = splitTree.getFocusedLeafId();
  if (!focusedLeafId) return "unavailable";
  splitTree.addSessionToLeaf(focusedLeafId, {
    ptyKey,
    label: filePath.split("/").pop() ?? filePath,
    icon: "file",
    type: "editor",
    filePath,
  });
  return "opened";
}
