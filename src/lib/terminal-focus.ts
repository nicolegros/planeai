/**
 * Terminal pane focus rules, extracted from App.svelte markup so the competing
 * invariants are testable in one place.
 *
 * Two rules are in tension:
 *
 * 1. A pane must only take focus when it belongs to the currently selected
 *    session. While a workspace layout is being swapped, the split tree still
 *    holds the previous workspace's tabs, and without this check the outgoing
 *    terminal reclaims focus mid-transition.
 *
 * 2. Terminal.svelte blurs xterm only when `focused` transitions true -> false.
 *    So if rule 1 leaves *no* pane focused while a terminal still owns DOM
 *    focus, that transition never fires and the terminal keeps swallowing keys
 *    forever — xterm calls stopPropagation(), so sidebar navigation never even
 *    reaches its window key handler.
 *
 * Rule 1 is safe only while the disagreement is transient. `reconcileRestoredLayout`
 * keeps it transient, and `shouldReleaseTerminalDomFocus` enforces rule 2
 * directly rather than relying on a prop transition that may never happen.
 */

export interface TerminalPaneFocusInput {
  /** This tab is the active tab of its leaf. */
  isActiveTabInLeaf: boolean;
  /** This tab's leaf is the focused leaf of the split tree. */
  isFocusedLeaf: boolean;
  /** This tab's session is the selected session (guards layout transitions). */
  belongsToActiveSession: boolean;
  /** The app-level keyboard focus zone. */
  zone: string;
  /** A plugin workspace is covering the workspace. */
  pluginOverlayActive: boolean;
  /** Any modal that must own the keyboard is open. */
  modalOpen: boolean;
}

export function isTerminalPaneFocused(input: TerminalPaneFocusInput): boolean {
  return (
    input.isActiveTabInLeaf &&
    input.belongsToActiveSession &&
    input.isFocusedLeaf &&
    input.zone === "terminal" &&
    !input.pluginOverlayActive &&
    !input.modalOpen
  );
}

/**
 * Whether a terminal currently holding DOM focus must be blurred.
 *
 * Deliberately independent of any pane's `focused` prop: the bug this guards
 * against is precisely the case where no pane is focused, so no prop transition
 * is available to trigger the blur.
 */
export function shouldReleaseTerminalDomFocus(input: {
  zone: string;
  domFocusInsideTerminal: boolean;
}): boolean {
  return input.zone !== "terminal" && input.domFocusInsideTerminal;
}

export interface RestoredLayoutReconciliation {
  /** Focus the tab belonging to the explicitly selected session. */
  focusTabForSession: boolean;
  /** Adopt the restored layout's active tab as the selected session. */
  adoptRestoredTabSession: boolean;
}

/**
 * A restored layout carries the active tab from when it was last saved — the
 * memory of which agent session you were last on in that task.
 *
 * `selectionIsExplicit` distinguishes a real user choice (clicking a session row
 * or tab) from an arbitrary entry point: `selectWorkspaceTask` passes the *first*
 * session linked to the task purely so there is something to load. Treating that
 * as a choice overrides the remembered tab, so returning to a task always lands
 * on its first session.
 *
 * Whichever side wins, the selected session and the visible tab must end up in
 * agreement — leaving them disagreeing strands the app in a state nothing later
 * reconciles.
 */
export function reconcileRestoredLayout(input: {
  restoredActiveTabSessionId: string | null;
  selectedSessionId: string;
  restoredTreeHasTabForSelectedSession: boolean;
  selectionIsExplicit: boolean;
}): RestoredLayoutReconciliation {
  const none = { focusTabForSession: false, adoptRestoredTabSession: false };
  if (input.restoredActiveTabSessionId === input.selectedSessionId) return none;

  if (input.restoredActiveTabSessionId === null) {
    // Nothing remembered — fall back to the selection if it has a tab.
    return input.restoredTreeHasTabForSelectedSession
      ? { focusTabForSession: true, adoptRestoredTabSession: false }
      : none;
  }

  // An arbitrary entry point never overrides the remembered tab.
  if (!input.selectionIsExplicit) {
    return { focusTabForSession: false, adoptRestoredTabSession: true };
  }

  return input.restoredTreeHasTabForSelectedSession
    ? { focusTabForSession: true, adoptRestoredTabSession: false }
    : { focusTabForSession: false, adoptRestoredTabSession: true };
}
