import type { LeafNode, TabEntry } from "./split-tree.svelte";

/** Extract the session ID from a pty key (strips ":tabIndex", ":diff", ":editor:path"). */
export function ptyKeyToSessionId(ptyKey: string): string {
  const colonIdx = ptyKey.indexOf(":");
  return colonIdx === -1 ? ptyKey : ptyKey.slice(0, colonIdx);
}

/** The split-tree operations workspace reconciliation depends on. */
export interface WorkspaceTabTree {
  getAllLeaves: () => LeafNode[];
  getFocusedLeaf: () => LeafNode | null;
  addSessionToLeaf: (leafId: string, tab: TabEntry) => void;
  setLeafActiveTab: (leafId: string, ptyKey: string) => void;
  removeSessionFromLeaf: (ptyKey: string) => boolean;
}

export interface WorkspaceTabReconcile {
  /** Tabs the TaskWorkspace should show, derived from its agent sessions. */
  workspaceEntries: TabEntry[];
  /** Sessions that still belong to the TaskWorkspace. */
  validSessionIds: ReadonlySet<string>;
  /**
   * Pty keys another flow has already reserved but not yet placed in the tree.
   * Terminal editor tabs reserve their key while the persisted tab count is in
   * flight; adding them here would mount a shell before its editor command is
   * queued, so the file would never open.
   */
  reservedPtyKeys?: { has: (ptyKey: string) => boolean };
  tree: WorkspaceTabTree;
}

/**
 * Reconcile restored/new TaskWorkspace agent sessions into the split tree and
 * prune archived or deleted ones, without replacing the current layout or
 * stealing the active tab.
 */
export function reconcileWorkspaceTabs(reconcile: WorkspaceTabReconcile): void {
  const { tree, reservedPtyKeys } = reconcile;
  const existingKeys = new Set(
    tree.getAllLeaves().flatMap((leaf) => leaf.tabs.map((tab) => tab.ptyKey)),
  );
  const missingEntries = reconcile.workspaceEntries.filter(
    (entry) => !existingKeys.has(entry.ptyKey) && !reservedPtyKeys?.has(entry.ptyKey),
  );
  const focusedLeaf = tree.getFocusedLeaf() ?? tree.getAllLeaves()[0];
  if (focusedLeaf && missingEntries.length > 0) {
    const activeTab = focusedLeaf.activeTab;
    for (const entry of missingEntries) tree.addSessionToLeaf(focusedLeaf.id, entry);
    if (activeTab) tree.setLeafActiveTab(focusedLeaf.id, activeTab);
  }

  const stalePtyKeys: string[] = [];
  for (const leaf of tree.getAllLeaves()) {
    for (const tab of leaf.tabs) {
      if (!reconcile.validSessionIds.has(ptyKeyToSessionId(tab.ptyKey))) {
        stalePtyKeys.push(tab.ptyKey);
      }
    }
  }
  for (const key of stalePtyKeys) tree.removeSessionFromLeaf(key);
}
