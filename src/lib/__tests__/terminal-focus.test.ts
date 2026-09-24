/**
 * Regression tests for the sidebar keyboard-navigation trap.
 *
 * State captured by instrumentation in the running app: `activeSessionId` was
 * session A while the focused leaf's active tab was session B's agent, and
 * session B's xterm textarea owned `document.activeElement`. No pane had
 * `focused: true`, so Terminal.svelte's true -> false blur never fired, session
 * B kept DOM focus through every zone change, and xterm's stopPropagation() meant
 * `k` never reached the sidebar's window key handler.
 */
import { describe, it, expect } from "vitest";
import {
  isTerminalPaneFocused,
  reconcileRestoredLayout,
  shouldReleaseTerminalDomFocus,
} from "../terminal-focus";

const base = {
  isActiveTabInLeaf: true,
  isFocusedLeaf: true,
  belongsToActiveSession: true,
  zone: "terminal",
  pluginOverlayActive: false,
  modalOpen: false,
};

describe("isTerminalPaneFocused", () => {
  it("focuses the focused leaf's active tab while the terminal zone is active", () => {
    expect(isTerminalPaneFocused(base)).toBe(true);
  });

  it("releases focus for every non-terminal zone", () => {
    for (const zone of ["sidebar", "editor", "explorer", "none"]) {
      expect(isTerminalPaneFocused({ ...base, zone })).toBe(false);
    }
  });

  it("does not focus a pane in an unfocused leaf or an inactive tab", () => {
    expect(isTerminalPaneFocused({ ...base, isFocusedLeaf: false })).toBe(false);
    expect(isTerminalPaneFocused({ ...base, isActiveTabInLeaf: false })).toBe(false);
  });

  it("does not let a pane from the previous layout reclaim focus mid-transition", () => {
    // Deliberate guard: during a workspace swap the split tree still holds the
    // outgoing workspace's tabs while the selected session has already moved.
    expect(isTerminalPaneFocused({ ...base, belongsToActiveSession: false })).toBe(false);
  });

  it("never focuses a pane behind a plugin overlay or a modal", () => {
    expect(isTerminalPaneFocused({ ...base, pluginOverlayActive: true })).toBe(false);
    expect(isTerminalPaneFocused({ ...base, modalOpen: true })).toBe(false);
  });
});

describe("shouldReleaseTerminalDomFocus", () => {
  it("releases xterm's DOM focus when the zone is not the terminal", () => {
    for (const zone of ["sidebar", "editor", "explorer", "none"]) {
      expect(shouldReleaseTerminalDomFocus({ zone, domFocusInsideTerminal: true })).toBe(true);
    }
  });

  it("leaves DOM focus alone while the terminal zone is active", () => {
    expect(shouldReleaseTerminalDomFocus({ zone: "terminal", domFocusInsideTerminal: true })).toBe(
      false,
    );
  });

  it("does nothing when no terminal holds DOM focus", () => {
    expect(shouldReleaseTerminalDomFocus({ zone: "sidebar", domFocusInsideTerminal: false })).toBe(
      false,
    );
  });

  it("releases focus even when the layout-transition guard leaves no pane focused", () => {
    // This is the bug. The pane owning DOM focus belongs to a different session
    // than the selected one, so `focused` is already false and stays false —
    // there is no true -> false transition for Terminal.svelte to act on.
    const strandedPane = { ...base, belongsToActiveSession: false };
    expect(isTerminalPaneFocused({ ...strandedPane, zone: "terminal" })).toBe(false);
    expect(isTerminalPaneFocused({ ...strandedPane, zone: "sidebar" })).toBe(false);

    // Releasing DOM focus must therefore not depend on that transition.
    expect(shouldReleaseTerminalDomFocus({ zone: "sidebar", domFocusInsideTerminal: true })).toBe(
      true,
    );
  });
});

describe("reconcileRestoredLayout", () => {
  const explicit = { selectionIsExplicit: true };
  const implicit = { selectionIsExplicit: false };

  it("does nothing when the restored tab already matches the selection", () => {
    expect(
      reconcileRestoredLayout({
        restoredActiveTabSessionId: "a",
        selectedSessionId: "a",
        restoredTreeHasTabForSelectedSession: true,
        ...explicit,
      }),
    ).toEqual({ focusTabForSession: false, adoptRestoredTabSession: false });
  });

  it("honours an explicit session pick over the remembered tab", () => {
    // Clicking a specific session row while another task workspace is loaded.
    expect(
      reconcileRestoredLayout({
        restoredActiveTabSessionId: "b",
        selectedSessionId: "a",
        restoredTreeHasTabForSelectedSession: true,
        ...explicit,
      }),
    ).toEqual({ focusTabForSession: true, adoptRestoredTabSession: false });
  });

  it("keeps the remembered tab when the session was only an arbitrary entry point", () => {
    // selectWorkspaceTask picks sessions.find(...) — the first session linked to
    // the task — purely to have something to load. That is not a user choice, so
    // the layout's remembered tab must win, otherwise returning to a task always
    // lands on its first session.
    expect(
      reconcileRestoredLayout({
        restoredActiveTabSessionId: "b",
        selectedSessionId: "a",
        restoredTreeHasTabForSelectedSession: true,
        ...implicit,
      }),
    ).toEqual({ focusTabForSession: false, adoptRestoredTabSession: true });
  });

  it("adopts the restored tab's session when an explicit selection has no tab", () => {
    expect(
      reconcileRestoredLayout({
        restoredActiveTabSessionId: "b",
        selectedSessionId: "a",
        restoredTreeHasTabForSelectedSession: false,
        ...explicit,
      }),
    ).toEqual({ focusTabForSession: false, adoptRestoredTabSession: true });
  });

  it("falls back to the selection when the restored layout has no active tab", () => {
    expect(
      reconcileRestoredLayout({
        restoredActiveTabSessionId: null,
        selectedSessionId: "a",
        restoredTreeHasTabForSelectedSession: true,
        ...implicit,
      }),
    ).toEqual({ focusTabForSession: true, adoptRestoredTabSession: false });
  });

  it("does nothing when there is neither a restored tab nor a tab for the selection", () => {
    expect(
      reconcileRestoredLayout({
        restoredActiveTabSessionId: null,
        selectedSessionId: "a",
        restoredTreeHasTabForSelectedSession: false,
        ...implicit,
      }),
    ).toEqual({ focusTabForSession: false, adoptRestoredTabSession: false });
  });

  it("never reports both actions at once", () => {
    for (const restored of ["a", "b", null]) {
      for (const hasTab of [true, false]) {
        for (const isExplicit of [true, false]) {
          const result = reconcileRestoredLayout({
            restoredActiveTabSessionId: restored,
            selectedSessionId: "a",
            restoredTreeHasTabForSelectedSession: hasTab,
            selectionIsExplicit: isExplicit,
          });
          expect(result.focusTabForSession && result.adoptRestoredTabSession).toBe(false);
        }
      }
    }
  });
});
