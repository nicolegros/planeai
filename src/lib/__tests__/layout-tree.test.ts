import { describe, expect, it } from "vitest";
import {
  activeTabOf,
  addTab,
  closePane,
  createLayout,
  cycleTab,
  destroyLeaf,
  findTab,
  focusDirection,
  focusTab,
  focusedLeafOf,
  focusedTabOf,
  leavesOf,
  moveFocusedTab,
  moveTab,
  neighborLeaf,
  reconcile,
  relabel,
  removeTab,
  restoreLayout,
  serializeLayout,
  setRatio,
  setTabTitle,
  splitFocused,
  splitWithTab,
  tabsOf,
  type Layout,
  type TabEntry,
} from "../layout-tree";

function agent(ptyKey: string, label = ptyKey): TabEntry {
  return { ptyKey, label, icon: "bot", type: "agent" };
}

function shell(ptyKey: string, label = "Shell"): TabEntry {
  return { ptyKey, label, icon: "terminal", type: "shell" };
}

function keys(layout: Layout | null): string[][] {
  return leavesOf(layout).map((leaf) => leaf.tabs.map((tab) => tab.ptyKey));
}

/** Left pane holding `left`, right pane holding `right`, right focused. */
function twoPanes(left: TabEntry[], right: TabEntry[]): Layout {
  const split = splitFocused(createLayout(left), "vertical")!;
  return right.reduce((layout, tab) => addTab(layout, split.leafId, tab), split.layout);
}

describe("createLayout", () => {
  it("makes one focused pane fronting the requested tab", () => {
    const layout = createLayout([agent("a"), agent("b")], "b");
    expect(keys(layout)).toEqual([["a", "b"]]);
    expect(focusedTabOf(layout)?.ptyKey).toBe("b");
  });

  it("drops repeated pty keys", () => {
    expect(keys(createLayout([agent("a"), agent("a"), agent("b")]))).toEqual([["a", "b"]]);
  });
});

describe("tabs", () => {
  it("adds a tab to a pane and brings it forward", () => {
    const layout = createLayout([agent("a")]);
    const next = addTab(layout, layout.focusedLeafId, shell("a:1"));
    expect(keys(next)).toEqual([["a", "a:1"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("a:1");
  });

  it("updates a pty key already in another pane in place instead of duplicating it", () => {
    const layout = twoPanes([agent("a"), shell("a:1")], [shell("a:2")]);
    const next = addTab(layout, layout.focusedLeafId, shell("a:1", "renamed"));
    expect(keys(next)).toEqual([["a", "a:1"], ["a:2"]]);
    expect(findTab(next, "a:1")?.tab.label).toBe("renamed");
    expect(findTab(next, "a:1")?.leaf.activeTab).toBe("a:1");
  });

  it("ignores a pane that is not in the tree", () => {
    const layout = createLayout([agent("a")]);
    expect(addTab(layout, "gone", shell("a:1"))).toBe(layout);
  });

  it("brings the neighbor forward when the front tab is removed", () => {
    const layout = createLayout([agent("a"), shell("a:1"), shell("a:2")], "a:1");
    const next = removeTab(layout, "a:1")!;
    expect(keys(next)).toEqual([["a", "a:2"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("a:2");
  });

  it("destroys a pane left empty, and the layout with its last pane", () => {
    const layout = twoPanes([agent("a")], [shell("a:1")]);
    const collapsed = removeTab(layout, "a:1")!;
    expect(keys(collapsed)).toEqual([["a"]]);
    expect(focusedTabOf(collapsed)?.ptyKey).toBe("a");
    expect(removeTab(collapsed, "a")).toBeNull();
  });

  it("falls back to the first tab when the front tab key is stale", () => {
    const layout = createLayout([agent("a"), agent("b")], "missing");
    expect(activeTabOf(focusedLeafOf(layout))?.ptyKey).toBe("a");
  });

  it("focuses a tab in another pane", () => {
    const layout = twoPanes([agent("a"), agent("b")], [shell("a:1")]);
    const next = focusTab(layout, "b");
    expect(focusedTabOf(next)?.ptyKey).toBe("b");
    expect(focusTab(next, "missing")).toBe(next);
  });

  it("cycles the focused pane's tabs in both directions", () => {
    const layout = createLayout([agent("a"), shell("a:1"), shell("a:2")], "a");
    expect(focusedTabOf(cycleTab(layout, 1))?.ptyKey).toBe("a:1");
    expect(focusedTabOf(cycleTab(layout, -1))?.ptyKey).toBe("a:2");
    const single = createLayout([agent("a")]);
    expect(cycleTab(single, 1)).toBe(single);
  });

  it("keeps custom titles when relabeling and skips unchanged layouts", () => {
    const titled = setTabTitle(createLayout([agent("a"), shell("a:1")]), "a:1", "nvim");
    const relabeled = relabel(
      titled,
      new Map([
        ["a", "Renamed"],
        ["a:1", "Shell"],
      ]),
    );
    expect(tabsOf(relabeled).map((tab) => tab.label)).toEqual(["Renamed", "nvim"]);
    expect(relabel(relabeled, new Map([["a", "Renamed"]]))).toBe(relabeled);
  });
});

describe("moving tabs", () => {
  it("moves a tab into another pane at an index and focuses it there", () => {
    const layout = twoPanes([agent("a"), shell("a:1")], [shell("a:2")]);
    const target = leavesOf(layout)[1].id;
    const next = moveTab(layout, "a:1", target, 0);
    expect(keys(next)).toEqual([["a"], ["a:1", "a:2"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("a:1");
  });

  it("reorders within a pane, counting the insert index before the drag", () => {
    const layout = createLayout([agent("a"), shell("a:1"), shell("a:2")]);
    expect(keys(moveTab(layout, "a", layout.focusedLeafId, 3))).toEqual([["a:1", "a:2", "a"]]);
    expect(keys(moveTab(layout, "a:2", layout.focusedLeafId, 0))).toEqual([["a:2", "a", "a:1"]]);
  });

  it("destroys the source pane when its last tab moves out", () => {
    const layout = twoPanes([agent("a")], [shell("a:1")]);
    const next = moveTab(layout, "a:1", leavesOf(layout)[0].id);
    expect(keys(next)).toEqual([["a", "a:1"]]);
    expect(next.tree.type).toBe("leaf");
  });

  it("moves the front tab to an existing neighbor, focus following", () => {
    const layout = focusTab(twoPanes([agent("a"), shell("a:1")], [shell("a:2")]), "a:1");
    const next = moveFocusedTab(layout, "right");
    expect(keys(next)).toEqual([["a"], ["a:2", "a:1"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("a:1");
  });

  it("splits on the requested side when there is no neighbor", () => {
    const layout = createLayout([agent("a"), shell("a:1")], "a:1");
    const next = moveFocusedTab(layout, "left");
    expect(keys(next)).toEqual([["a:1"], ["a"]]);
    expect(next.tree.type === "split" && next.tree.direction).toBe("vertical");
    expect(focusedTabOf(next)?.ptyKey).toBe("a:1");
    expect(moveFocusedTab(createLayout([agent("a")]), "left").tree.type).toBe("leaf");
  });
});

describe("splitting with a dragged tab", () => {
  it("splits a pane on the side the tab was dropped on", () => {
    const layout = twoPanes([agent("a"), shell("a:1")], [shell("a:2")]);
    const right = leavesOf(layout)[1].id;
    const next = splitWithTab(layout, "a:1", right, "down");
    expect(keys(next)).toEqual([["a"], ["a:2"], ["a:1"]]);
    const rightSplit = next.tree.type === "split" ? next.tree.children[1] : null;
    expect(rightSplit?.type === "split" && rightSplit.direction).toBe("horizontal");
    expect(focusedTabOf(next)?.ptyKey).toBe("a:1");
  });

  it("splits a pane off one of its own tabs, before it for left and up", () => {
    const layout = createLayout([agent("a"), shell("a:1")]);
    const next = splitWithTab(layout, "a:1", layout.focusedLeafId, "left");
    expect(keys(next)).toEqual([["a:1"], ["a"]]);
  });

  it("removes a source pane emptied by the drag", () => {
    const layout = twoPanes([agent("a"), shell("a:1")], [shell("a:2")]);
    const left = leavesOf(layout)[0].id;
    const next = splitWithTab(layout, "a:2", left, "right");
    expect(keys(next)).toEqual([["a", "a:1"], ["a:2"]]);
  });

  it("will not split a pane off its only tab", () => {
    const layout = createLayout([agent("a")]);
    expect(splitWithTab(layout, "a", layout.focusedLeafId, "right")).toBe(layout);
  });
});

describe("panes", () => {
  it("splits the focused pane, keeping its tabs first and focusing the new pane", () => {
    const split = splitFocused(createLayout([agent("a")]), "horizontal")!;
    expect(keys(split.layout)).toEqual([["a"], []]);
    expect(split.layout.focusedLeafId).toBe(split.leafId);
  });

  it("closes a pane by migrating its tabs to its sibling", () => {
    const layout = twoPanes([agent("a")], [shell("a:1"), shell("a:2")]);
    const next = closePane(layout, layout.focusedLeafId);
    expect(keys(next)).toEqual([["a", "a:1", "a:2"]]);
    expect(focusedLeafOf(next)?.id).toBe(leavesOf(next)[0].id);
  });

  it("cannot close the only pane", () => {
    const layout = createLayout([agent("a")]);
    expect(closePane(layout, layout.focusedLeafId)).toBe(layout);
  });

  it("destroys a pane with its tabs and focuses its sibling", () => {
    const layout = twoPanes([agent("a")], [shell("a:1")]);
    const next = destroyLeaf(layout, layout.focusedLeafId)!;
    expect(keys(next)).toEqual([["a"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("a");
  });

  it("clamps split ratios", () => {
    const layout = twoPanes([agent("a")], [shell("a:1")]);
    const splitId = layout.tree.id;
    const ratio = (next: Layout) => (next.tree.type === "split" ? next.tree.ratio : null);
    expect(ratio(setRatio(layout, splitId, 0.7))).toBe(0.7);
    expect(ratio(setRatio(layout, splitId, 0))).toBe(0.1);
    expect(ratio(setRatio(layout, splitId, 2))).toBe(0.9);
    expect(setRatio(layout, splitId, 0.5)).toBe(layout);
  });

  it("navigates to visual neighbors", () => {
    const layout = twoPanes([agent("a")], [shell("a:1")]);
    expect(neighborLeaf(layout, "left")?.tabs[0].ptyKey).toBe("a");
    expect(neighborLeaf(layout, "right")).toBeNull();
    expect(neighborLeaf(layout, "down")).toBeNull();
    expect(focusedTabOf(focusDirection(layout, "left"))?.ptyKey).toBe("a");
    const stacked = splitFocused(createLayout([agent("a")]), "horizontal")!.layout;
    expect(neighborLeaf(stacked, "up")?.tabs[0].ptyKey).toBe("a");
  });
});

describe("reconcile", () => {
  it("adds new agents to the focused pane without stealing the front tab", () => {
    const next = reconcile(createLayout([agent("a")]), { agentTabs: [agent("a"), agent("b")] })!;
    expect(keys(next)).toEqual([["a", "b"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("a");
  });

  it("drops every tab of a session that left the workspace", () => {
    const layout = twoPanes(
      [agent("a"), agent("b"), shell("b:1")],
      [shell("b:2"), { ptyKey: "b:diff", label: "Diff", icon: "git-compare", type: "diff" }],
    );
    expect(keys(reconcile(layout, { agentTabs: [agent("a")] }))).toEqual([["a"]]);
  });

  it("keeps shell tabs of sessions that remain", () => {
    const layout = createLayout([agent("a"), shell("a:1", "foo.ts")]);
    const next = reconcile(layout, { agentTabs: [agent("a")] });
    expect(next).toBe(layout);
  });

  it("relabels agents but keeps custom titles", () => {
    const layout = setTabTitle(createLayout([agent("a"), agent("b")]), "b", "Mine");
    const next = reconcile(layout, {
      agentTabs: [agent("a", "Fix login (2)"), agent("b", "Other")],
    })!;
    expect(tabsOf(next).map((tab) => tab.label)).toEqual(["Fix login (2)", "Mine"]);
  });

  it("fronts the preferred tab when the pane has no front tab of its own", () => {
    // A layout saved from an emptied workspace persists activeTab: "".
    const layout = restoreLayout(
      JSON.stringify({
        tree: { type: "leaf", id: "l", tabs: [], activeTab: "" },
        focusedLeafId: "l",
      }),
    );
    const next = reconcile(layout, {
      agentTabs: [agent("selected"), agent("other")],
      preferredActiveTab: "selected",
    });
    expect(focusedTabOf(next)?.ptyKey).toBe("selected");
  });

  it("fronts a tab just added for the preferred session over the remembered one", () => {
    const next = reconcile(createLayout([agent("remembered")]), {
      agentTabs: [agent("remembered"), agent("selected")],
      preferredActiveTab: "selected",
    });
    expect(focusedTabOf(next)?.ptyKey).toBe("selected");
  });

  it("does not move the user when the preferred session was already present", () => {
    const next = reconcile(createLayout([agent("remembered"), agent("selected")]), {
      agentTabs: [agent("remembered"), agent("selected"), agent("newcomer")],
      preferredActiveTab: "selected",
    });
    expect(focusedTabOf(next)?.ptyKey).toBe("remembered");
  });

  it("rebuilds a layout emptied by the last agent leaving", () => {
    const next = reconcile(null, { agentTabs: [agent("a"), agent("b")], preferredActiveTab: "b" });
    expect(keys(next)).toEqual([["a", "b"]]);
    expect(focusedTabOf(next)?.ptyKey).toBe("b");
    expect(reconcile(null, { agentTabs: [] })).toBeNull();
  });
});

describe("persistence", () => {
  it("round-trips a split layout", () => {
    const layout = setTabTitle(twoPanes([agent("a"), shell("a:3")], [shell("a:1")]), "a:3", "nvim");
    expect(restoreLayout(serializeLayout(layout))).toEqual(layout);
  });

  it("drops pty keys repeated across panes and repairs the front tab", () => {
    const json = JSON.stringify({
      tree: {
        type: "split",
        id: "s",
        direction: "vertical",
        ratio: 0.5,
        children: [
          { type: "leaf", id: "l1", tabs: [agent("a"), shell("a:1")], activeTab: "a" },
          { type: "leaf", id: "l2", tabs: [shell("a:1"), shell("a:1")], activeTab: "a:1" },
        ],
      },
      focusedLeafId: "l2",
    });
    const restored = restoreLayout(json)!;
    expect(keys(restored)).toEqual([["a", "a:1"], []]);
    expect(focusedLeafOf(restored)?.activeTab).toBe("");
  });

  it("infers the type of tabs saved before types existed", () => {
    const json = JSON.stringify({
      tree: {
        type: "leaf",
        id: "l",
        activeTab: "a",
        tabs: [
          { ptyKey: "a" },
          { ptyKey: "a:2" },
          { ptyKey: "a:diff" },
          { ptyKey: "a:editor:src/x.ts" },
        ],
      },
      focusedLeafId: "l",
    });
    const tabs = tabsOf(restoreLayout(json));
    expect(tabs.map((tab) => tab.type)).toEqual(["agent", "shell", "diff", "editor"]);
    expect(tabs[3].filePath).toBe("src/x.ts");
  });

  it("falls back to the first pane when the focused one is gone", () => {
    const json = JSON.stringify({
      tree: { type: "leaf", id: "l", tabs: [agent("a")], activeTab: "a" },
      focusedLeafId: "gone",
    });
    expect(restoreLayout(json)?.focusedLeafId).toBe("l");
  });

  it("rejects malformed layouts", () => {
    const leaf = { type: "leaf", id: "l", tabs: [], activeTab: "" };
    for (const data of [
      "not json",
      JSON.stringify(null),
      JSON.stringify({ tree: leaf }),
      JSON.stringify({ tree: { ...leaf, tabs: "x" }, focusedLeafId: "l" }),
      JSON.stringify({
        tree: { type: "split", id: "s", direction: "vertical", ratio: 2, children: [leaf, leaf] },
        focusedLeafId: "l",
      }),
      JSON.stringify({
        tree: { type: "split", id: "s", direction: "diagonal", ratio: 0.5, children: [leaf, leaf] },
        focusedLeafId: "l",
      }),
      JSON.stringify({
        tree: { type: "split", id: "s", direction: "vertical", ratio: 0.5, children: [leaf] },
        focusedLeafId: "l",
      }),
    ]) {
      expect(restoreLayout(data), data).toBeNull();
    }
  });

  it("drops tabs whose pty key is not in the grammar", () => {
    const json = JSON.stringify({
      tree: { type: "leaf", id: "l", tabs: [agent("a"), shell("a:x")], activeTab: "a" },
      focusedLeafId: "l",
    });
    expect(keys(restoreLayout(json))).toEqual([["a"]]);
  });
});
