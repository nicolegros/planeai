/**
 * Two failure modes, both seen in practice: overriding the remembered tab with an
 * arbitrary entry point (so returning to a task always lands on its first
 * session), and leaving selection and visible tab disagreeing — which strands
 * xterm's DOM focus. See resolveRestoredLayoutSelection in ../workspace-tabs.
 */
import { describe, it, expect } from "vitest";
import { resolveRestoredLayoutSelection, reconcileWorkspaceTabs } from "../workspace-tabs";
import type { LeafNode, TabEntry } from "../split-tree.svelte";

const base = {
  liveRestoredSessionId: "remembered" as string | null,
  selectedSessionId: "selected",
  restoredTreeHasTabForSelectedSession: true,
  selectionIsExplicit: true,
};

describe("resolveRestoredLayoutSelection", () => {
  it("does nothing when the remembered tab already matches the selection", () => {
    expect(resolveRestoredLayoutSelection({ ...base, liveRestoredSessionId: "selected" })).toEqual({
      kind: "keep_selection",
    });
  });

  it("keeps the remembered tab when the selection was an arbitrary entry point", () => {
    expect(resolveRestoredLayoutSelection({ ...base, selectionIsExplicit: false })).toEqual({
      kind: "adopt_restored_session",
      sessionId: "remembered",
    });
  });

  it("honours an explicit pick over the remembered tab", () => {
    expect(resolveRestoredLayoutSelection(base)).toEqual({ kind: "focus_selected_session" });
  });

  it("never adopts a remembered tab whose session is gone", () => {
    // Deleting a session in another task leaves its pty key in that task's saved
    // layout. The caller passes null for it, so adoption cannot select a session
    // that no longer exists — which would stall every later repair.
    expect(
      resolveRestoredLayoutSelection({
        ...base,
        liveRestoredSessionId: null,
        selectionIsExplicit: false,
      }),
    ).toEqual({ kind: "focus_selected_session" });
  });

  it("never discards an explicit selection that has no tab yet", () => {
    // Creating a session for another task selects it before its tab exists.
    // Adopting the remembered tab here would silently drop the new session.
    expect(
      resolveRestoredLayoutSelection({ ...base, restoredTreeHasTabForSelectedSession: false }),
    ).toEqual({ kind: "keep_selection" });
  });

  it("keeps the selection when the layout remembers no live tab", () => {
    expect(
      resolveRestoredLayoutSelection({
        ...base,
        liveRestoredSessionId: null,
        selectionIsExplicit: false,
      }),
    ).toEqual({ kind: "focus_selected_session" });
  });

  it("only ever adopts a live, remembered, non-explicit selection", () => {
    for (const remembered of ["remembered", null]) {
      for (const hasTab of [true, false]) {
        for (const isExplicit of [true, false]) {
          const result = resolveRestoredLayoutSelection({
            liveRestoredSessionId: remembered,
            selectedSessionId: "selected",
            restoredTreeHasTabForSelectedSession: hasTab,
            selectionIsExplicit: isExplicit,
          });
          if (result.kind === "adopt_restored_session") {
            expect(remembered).not.toBeNull();
            expect(isExplicit).toBe(false);
          }
        }
      }
    }
  });
});

describe("reconcileWorkspaceTabs preferredActiveTab", () => {
  function tab(ptyKey: string): TabEntry {
    return { ptyKey, type: "agent", label: ptyKey } as TabEntry;
  }

  function fakeTree(activeTab: string) {
    const leaf: LeafNode = {
      type: "leaf",
      id: "leaf-1",
      tabs: [],
      activeTab,
    } as unknown as LeafNode;
    return {
      leaf,
      getAllLeaves: () => [leaf],
      getFocusedLeaf: () => leaf,
      addSessionToLeaf: (_leafId: string, entry: TabEntry) => {
        leaf.tabs.push(entry);
        (leaf as { activeTab: string }).activeTab = entry.ptyKey;
      },
      setLeafActiveTab: (_leafId: string, ptyKey: string) => {
        (leaf as { activeTab: string }).activeTab = ptyKey;
      },
      removeSessionFromLeaf: () => true,
    };
  }

  it("activates the preferred tab when the restored leaf has no active tab", () => {
    // A layout saved from an emptied workspace persists activeTab: "". Without a
    // preference the last tab added wins, which need not be the selected session.
    const tree = fakeTree("");
    reconcileWorkspaceTabs({
      workspaceEntries: [tab("selected"), tab("other")],
      validSessionIds: new Set(["selected", "other"]),
      preferredActiveTab: "selected",
      tree,
    });
    expect(tree.leaf.activeTab).toBe("selected");
  });

  it("activates a tab just added for the preferred session over the remembered one", () => {
    // Selecting a newly created session for another task: the layout remembers a
    // live tab, and the new session's tab only appears here. Leaving the
    // remembered tab active would hide the session the user just created and
    // strand selection and visible tab in disagreement.
    const tree = fakeTree("remembered");
    tree.leaf.tabs.push(tab("remembered"));
    reconcileWorkspaceTabs({
      workspaceEntries: [tab("remembered"), tab("selected")],
      validSessionIds: new Set(["remembered", "selected"]),
      preferredActiveTab: "selected",
      tree,
    });
    expect(tree.leaf.activeTab).toBe("selected");
  });

  it("does not steal the active tab when the preferred session was already present", () => {
    // Unrelated sessions appearing in a loaded workspace must not move the user.
    const tree = fakeTree("remembered");
    tree.leaf.tabs.push(tab("remembered"), tab("selected"));
    reconcileWorkspaceTabs({
      workspaceEntries: [tab("remembered"), tab("selected"), tab("newcomer")],
      validSessionIds: new Set(["remembered", "selected", "newcomer"]),
      preferredActiveTab: "selected",
      tree,
    });
    expect(tree.leaf.activeTab).toBe("remembered");
  });
});
