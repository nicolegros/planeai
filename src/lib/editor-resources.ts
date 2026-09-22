/**
 * Editor resources — connects the embedded editor tabs in the TaskWorkspace
 * layout to the app-level commands that act on them.
 *
 * The split tree knows which editor tab is in front of the user; only the mounted
 * EditorTab knows how to save its buffer. Each tab registers itself under its pty
 * key so commands like Cmd+S can reach the right one.
 */
import * as splitTree from "./split-tree.svelte";

export interface EditorResourceHandle {
  save: () => void | Promise<void>;
}

const handles = new Map<string, EditorResourceHandle>();

/** Register a mounted editor tab under its pty key. */
export function registerEditorResource(ptyKey: string, handle: EditorResourceHandle): void {
  handles.set(ptyKey, handle);
}

/** Drop an editor tab's handle when it unmounts. */
export function unregisterEditorResource(ptyKey: string): void {
  handles.delete(ptyKey);
}

/** Pty key of the editor tab the user is looking at, if the front tab is an editor. */
export function activeEditorResourceKey(): string | null {
  const leaf = splitTree.getFocusedLeaf();
  if (!leaf) return null;
  const active = splitTree.getActiveTabEntry(leaf);
  return active?.type === "editor" ? active.ptyKey : null;
}

/**
 * Save the editor tab in the focused pane. Returns false when that pane is not
 * showing an editor, or when its tab has not mounted yet.
 */
export function saveActiveEditorResource(): boolean {
  const ptyKey = activeEditorResourceKey();
  if (!ptyKey) return false;
  const handle = handles.get(ptyKey);
  if (!handle) return false;
  void handle.save();
  return true;
}

/** Reset registered handles (for tests). */
export function _resetEditorResources(): void {
  handles.clear();
}
