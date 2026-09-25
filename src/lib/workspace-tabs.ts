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
  relabelTabs: (labels: ReadonlyMap<string, string>) => void;
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
  /**
   * Tab the workspace was loaded for. It wins when it is among the tabs added
   * here, or when the leaf has no active tab of its own (a restored layout can
   * carry an empty `activeTab`); otherwise the leaf keeps its active tab so
   * unrelated sessions appearing later cannot move the user.
   */
  preferredActiveTab?: string;
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
    const previousActiveTab = focusedLeaf.activeTab;
    for (const entry of missingEntries) tree.addSessionToLeaf(focusedLeaf.id, entry);
    // A tab just added for the preferred session wins over the remembered one:
    // the workspace was loaded for that session, and keeping the remembered tab
    // active would strand selection and visible tab in disagreement.
    const preferred = reconcile.preferredActiveTab;
    const preferredWasAdded = !!preferred && missingEntries.some((e) => e.ptyKey === preferred);
    const activeTab = preferredWasAdded ? preferred : previousActiveTab || preferred;
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

  // Labels are copied into the tree (and persisted layout) when a tab is added.
  tree.relabelTabs(new Map(reconcile.workspaceEntries.map((entry) => [entry.ptyKey, entry.label])));
}

/**
 * What a restored layout implies for the session selection. Its saved active tab
 * is the memory of which agent you were last on, and need not be the session the
 * load started for. The two must not be left disagreeing: nothing later
 * reconciles them, and while they disagree no terminal pane qualifies as focused
 * (see `isTerminalPaneFocused` in ./terminal-focus).
 */
export type RestoredLayoutSelection =
  | { kind: "keep_selection" }
  | { kind: "focus_selected_session" }
  | { kind: "adopt_restored_session"; sessionId: string };

export function resolveRestoredLayoutSelection(input: {
  /**
   * Session of the restored layout's active tab, but only when it still exists in
   * this workspace. A remembered session that is gone must never be adopted: it
   * would select a session nothing can resolve, stalling every later repair.
   */
  liveRestoredSessionId: string | null;
  selectedSessionId: string;
  restoredTreeHasTabForSelectedSession: boolean;
  /**
   * A user choice, rather than an arbitrary entry point: `selectWorkspaceTask`
   * passes the *first* session linked to the task purely so there is something to
   * load, and treating that as a choice always lands on the task's first session.
   */
  selectionIsExplicit: boolean;
}): RestoredLayoutSelection {
  const selectionWins: RestoredLayoutSelection = input.restoredTreeHasTabForSelectedSession
    ? { kind: "focus_selected_session" }
    : // No tab yet — reconcileWorkspaceTabs will add one, and `preferredActiveTab`
      // makes it active. Adopting here would silently discard the selection.
      { kind: "keep_selection" };

  if (input.liveRestoredSessionId === input.selectedSessionId) return { kind: "keep_selection" };
  if (input.liveRestoredSessionId === null) return selectionWins;
  // A real user choice is never overridden by the remembered tab.
  if (input.selectionIsExplicit) return selectionWins;
  return { kind: "adopt_restored_session", sessionId: input.liveRestoredSessionId };
}
