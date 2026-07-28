import { describe, it, expect, beforeEach } from "vitest";
import {
  initTree,
  resetTree,
  getTree,
  getFocusedLeafId,
  getFocusedLeaf,
  getAllLeaves,
  splitFocusedLeaf,
  addSessionToLeaf,
  removeSessionFromLeaf,
  moveSessionToLeaf,
  closeSplit,
  destroyLeaf,
  setFocusedLeaf,
  setLeafActiveTab,
  setRatio,
  focusDirection,
  moveTabToDirection,
  getNeighborLeaf,
  getLeafForSession,
  getActiveTabEntry,
  updateTabLabel,
  serialize,
  deserialize,
  _resetIdCounter,
} from "../split-tree.svelte";
import type { TabEntry } from "../split-tree.svelte";

function tab(ptyKey: string, label?: string): TabEntry {
  return { ptyKey, label: label ?? ptyKey.toUpperCase(), icon: "terminal" };
}

beforeEach(() => {
  resetTree();
  _resetIdCounter();
});

describe("initTree", () => {
  it("creates a single leaf with given tabs", () => {
    initTree([tab("s1"), tab("s2")]);
    const tree = getTree();
    expect(tree).not.toBeNull();
    expect(tree!.type).toBe("leaf");
    if (tree!.type === "leaf") {
      expect(tree!.tabs).toHaveLength(2);
      expect(tree!.tabs[0].ptyKey).toBe("s1");
      expect(tree!.tabs[1].ptyKey).toBe("s2");
      expect(tree!.activeTab).toBe("s1");
    }
    expect(getFocusedLeafId()).toBe(tree!.id);
  });

  it("respects activeTab parameter", () => {
    initTree([tab("s1"), tab("s2"), tab("s3")], "s3");
    const leaf = getFocusedLeaf();
    expect(leaf!.activeTab).toBe("s3");
  });
});

describe("splitFocusedLeaf", () => {
  it("splits into a binary tree with original tabs in first child", () => {
    initTree([tab("s1"), tab("s2")]);
    const originalLeafId = getFocusedLeafId();
    const newLeafId = splitFocusedLeaf("vertical");

    expect(newLeafId).not.toBeNull();
    const tree = getTree();
    expect(tree!.type).toBe("split");
    if (tree!.type === "split") {
      expect(tree!.direction).toBe("vertical");
      expect(tree!.ratio).toBe(0.5);
      expect(tree!.children[0].type).toBe("leaf");
      expect(tree!.children[1].type).toBe("leaf");

      const first = tree!.children[0] as { type: "leaf"; id: string; tabs: TabEntry[] };
      const second = tree!.children[1] as { type: "leaf"; id: string; tabs: TabEntry[] };
      expect(first.tabs.map((t) => t.ptyKey)).toEqual(["s1", "s2"]);
      expect(first.id).toBe(originalLeafId);
      expect(second.tabs).toEqual([]);
      expect(second.id).toBe(newLeafId);
    }
  });

  it("moves focus to the new leaf", () => {
    initTree([tab("s1")]);
    const newLeafId = splitFocusedLeaf("horizontal");
    expect(getFocusedLeafId()).toBe(newLeafId);
  });

  it("supports nested splits (recursive binary tree)", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    addSessionToLeaf(getFocusedLeafId()!, tab("s2"));
    splitFocusedLeaf("horizontal");

    const leaves = getAllLeaves();
    expect(leaves.length).toBe(3);
  });

  it("returns null when no tree exists", () => {
    expect(splitFocusedLeaf("vertical")).toBeNull();
  });
});

describe("addSessionToLeaf", () => {
  it("appends tab and updates activeTab", () => {
    initTree([tab("s1")]);
    const leafId = getFocusedLeafId()!;
    addSessionToLeaf(leafId, tab("s2"));

    const leaf = getFocusedLeaf()!;
    expect(leaf.tabs.map((t) => t.ptyKey)).toEqual(["s1", "s2"]);
    expect(leaf.activeTab).toBe("s2");
  });
});

describe("removeSessionFromLeaf", () => {
  it("removes tab and keeps leaf alive if tabs remain", () => {
    initTree([tab("s1"), tab("s2")]);
    const destroyed = removeSessionFromLeaf("s1");
    expect(destroyed).toBe(false);
    const leaf = getFocusedLeaf()!;
    expect(leaf.tabs.map((t) => t.ptyKey)).toEqual(["s2"]);
  });

  it("destroys leaf when last tab is removed", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    const emptyLeafId = getFocusedLeafId()!;
    addSessionToLeaf(emptyLeafId, tab("s2"));

    // Now we have: split -> [leaf(s1), leaf(s2)]
    setFocusedLeaf(emptyLeafId);
    const destroyed = removeSessionFromLeaf("s2");
    expect(destroyed).toBe(true);

    // Tree should collapse to single leaf
    const tree = getTree();
    expect(tree!.type).toBe("leaf");
  });
});

describe("moveSessionToLeaf", () => {
  it("moves a tab from one leaf to another", () => {
    initTree([tab("s1"), tab("s2")]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, tab("s3"));

    moveSessionToLeaf("s1", newLeafId);

    const leaves = getAllLeaves();
    const original = leaves.find((l) => l.id === originalLeafId)!;
    const target = leaves.find((l) => l.id === newLeafId)!;
    expect(original.tabs.map((t) => t.ptyKey)).toEqual(["s2"]);
    expect(target.tabs.map((t) => t.ptyKey)).toContain("s1");
    expect(target.tabs.map((t) => t.ptyKey)).toContain("s3");
  });

  it("moves at specific insertIndex", () => {
    initTree([tab("s1"), tab("s2")]);
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, tab("s3"));
    addSessionToLeaf(newLeafId, tab("s4"));

    moveSessionToLeaf("s1", newLeafId, 1);

    const target = getAllLeaves().find((l) => l.id === newLeafId)!;
    expect(target.tabs.map((t) => t.ptyKey)).toEqual(["s3", "s1", "s4"]);
  });

  it("destroys source leaf when last tab is moved out", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, tab("s2"));

    moveSessionToLeaf("s1", newLeafId);

    // Original leaf should be destroyed, tree collapses
    const tree = getTree();
    expect(tree!.type).toBe("leaf");
    if (tree!.type === "leaf") {
      expect(tree!.tabs.map((t) => t.ptyKey)).toContain("s1");
      expect(tree!.tabs.map((t) => t.ptyKey)).toContain("s2");
    }
  });
});

describe("closeSplit", () => {
  it("migrates tabs to sibling and destroys the split", () => {
    initTree([tab("s1")]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, tab("s2"));
    addSessionToLeaf(newLeafId, tab("s3"));

    closeSplit(newLeafId);

    const tree = getTree();
    expect(tree!.type).toBe("leaf");
    if (tree!.type === "leaf") {
      expect(tree!.tabs.map((t) => t.ptyKey)).toEqual(["s1", "s2", "s3"]);
    }
  });

  it("does nothing on root leaf", () => {
    initTree([tab("s1")]);
    closeSplit(getFocusedLeafId()!);
    expect(getTree()!.type).toBe("leaf");
  });
});

describe("destroyLeaf", () => {
  it("collapses tree when leaf is destroyed", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;

    destroyLeaf(newLeafId);

    const tree = getTree();
    expect(tree!.type).toBe("leaf");
  });

  it("clears tree when last leaf is destroyed", () => {
    initTree([tab("s1")]);
    destroyLeaf(getFocusedLeafId()!);
    expect(getTree()).toBeNull();
    expect(getFocusedLeafId()).toBeNull();
  });

  it("updates focus to sibling", () => {
    initTree([tab("s1")]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;

    destroyLeaf(newLeafId);
    expect(getFocusedLeafId()).toBe(originalLeafId);
  });
});

describe("setLeafActiveTab", () => {
  it("sets active tab by ptyKey", () => {
    initTree([tab("s1"), tab("s2"), tab("s3")]);
    setLeafActiveTab(getFocusedLeafId()!, "s3");
    expect(getFocusedLeaf()!.activeTab).toBe("s3");
  });

  it("does nothing for unknown ptyKey", () => {
    initTree([tab("s1"), tab("s2")]);
    setLeafActiveTab(getFocusedLeafId()!, "unknown");
    // activeTab stays as initialized (s1)
    expect(getFocusedLeaf()!.activeTab).toBe("s1");
  });
});

describe("setRatio", () => {
  it("updates the split ratio", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    const tree = getTree()!;
    if (tree.type === "split") {
      setRatio(tree.id, 0.7);
      const updated = getTree()!;
      if (updated.type === "split") {
        expect(updated.ratio).toBe(0.7);
      }
    }
  });

  it("clamps to [0.1, 0.9]", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    const tree = getTree()!;
    if (tree.type === "split") {
      setRatio(tree.id, 0.0);
      const t1 = getTree()!;
      if (t1.type === "split") expect(t1.ratio).toBe(0.1);

      setRatio(tree.id, 1.0);
      const t2 = getTree()!;
      if (t2.type === "split") expect(t2.ratio).toBe(0.9);
    }
  });
});

describe("spatial navigation", () => {
  it("finds right neighbor in vertical split", () => {
    initTree([tab("s1")]);
    const leftLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");

    setFocusedLeaf(leftLeafId);
    const neighbor = getNeighborLeaf("right");
    expect(neighbor).not.toBeNull();
    expect(neighbor!.tabs).toEqual([]);
  });

  it("finds left neighbor in vertical split", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");

    const neighbor = getNeighborLeaf("left");
    expect(neighbor).not.toBeNull();
    expect(neighbor!.tabs.map((t) => t.ptyKey)).toEqual(["s1"]);
  });

  it("finds down neighbor in horizontal split", () => {
    initTree([tab("s1")]);
    const topLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("horizontal");

    setFocusedLeaf(topLeafId);
    const neighbor = getNeighborLeaf("down");
    expect(neighbor).not.toBeNull();
  });

  it("returns null when no neighbor exists", () => {
    initTree([tab("s1")]);
    expect(getNeighborLeaf("left")).toBeNull();
    expect(getNeighborLeaf("right")).toBeNull();
  });

  it("focusDirection changes focused leaf", () => {
    initTree([tab("s1")]);
    const leftId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const rightId = getFocusedLeafId()!;

    setFocusedLeaf(leftId);
    const result = focusDirection("right");
    expect(result).toBe(rightId);
    expect(getFocusedLeafId()).toBe(rightId);
  });
});

describe("moveTabToDirection", () => {
  it("moves tab to existing neighbor", () => {
    initTree([tab("s1"), tab("s2")]);
    const leftId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const rightId = getFocusedLeafId()!;
    addSessionToLeaf(rightId, tab("s3"));

    setFocusedLeaf(leftId);
    setLeafActiveTab(leftId, "s1");

    moveTabToDirection("right");

    const rightLeaf = getAllLeaves().find((l) => l.id === rightId);
    expect(rightLeaf!.tabs.map((t) => t.ptyKey)).toContain("s1");
  });

  it("creates a new split when no neighbor exists", () => {
    initTree([tab("s1"), tab("s2")]);
    setLeafActiveTab(getFocusedLeafId()!, "s1");

    const result = moveTabToDirection("right");
    expect(result).not.toBeNull();

    const leaves = getAllLeaves();
    expect(leaves.length).toBe(2);
    const newLeaf = leaves.find((l) => l.tabs.some((t) => t.ptyKey === "s1"));
    expect(newLeaf).not.toBeNull();
    const originalLeaf = leaves.find((l) => l.tabs.some((t) => t.ptyKey === "s2"));
    expect(originalLeaf).not.toBeNull();
  });
});

describe("getLeafForSession", () => {
  it("finds the leaf containing a ptyKey", () => {
    initTree([tab("s1"), tab("s2")]);
    const leaf = getLeafForSession("s1");
    expect(leaf).not.toBeNull();
    expect(leaf!.tabs.map((t) => t.ptyKey)).toContain("s1");
  });

  it("returns null for unknown ptyKey", () => {
    initTree([tab("s1")]);
    expect(getLeafForSession("unknown")).toBeNull();
  });
});

describe("getActiveTabEntry", () => {
  it("returns the active tab entry", () => {
    initTree([tab("s1"), tab("s2")], "s2");
    const leaf = getFocusedLeaf()!;
    const entry = getActiveTabEntry(leaf);
    expect(entry).not.toBeNull();
    expect(entry!.ptyKey).toBe("s2");
  });

  it("returns first tab if activeTab not found", () => {
    initTree([tab("s1"), tab("s2")]);
    const leaf = getFocusedLeaf()!;
    const entry = getActiveTabEntry(leaf);
    expect(entry!.ptyKey).toBe("s1");
  });
});

describe("updateTabLabel", () => {
  it("updates the label and sets customTitle", () => {
    initTree([tab("s1", "Shell"), tab("s2", "Shell")]);
    updateTabLabel("s1", "vim");
    const leaf = getFocusedLeaf()!;
    const t = leaf.tabs.find((t) => t.ptyKey === "s1")!;
    expect(t.label).toBe("vim");
    expect(t.customTitle).toBe(true);
  });

  it("works across nested leaves", () => {
    initTree([tab("s1")]);
    splitFocusedLeaf("vertical");
    addSessionToLeaf(getFocusedLeafId()!, tab("s2", "Shell"));

    updateTabLabel("s2", "git");
    const leaves = getAllLeaves();
    const leaf = leaves.find((l) => l.tabs.some((t) => t.ptyKey === "s2"))!;
    const t = leaf.tabs.find((t) => t.ptyKey === "s2")!;
    expect(t.label).toBe("git");
    expect(t.customTitle).toBe(true);
  });
});

describe("serialize / deserialize", () => {
  it("round-trips the tree", () => {
    initTree([tab("s1"), tab("s2")]);
    splitFocusedLeaf("vertical");
    addSessionToLeaf(getFocusedLeafId()!, tab("s3"));

    const data = serialize()!;
    expect(data).not.toBeNull();
    expect(data.tree.type).toBe("split");

    resetTree();
    expect(getTree()).toBeNull();

    deserialize(data);
    expect(getTree()!.type).toBe("split");
    const leaves = getAllLeaves();
    expect(leaves.length).toBe(2);
  });
});
