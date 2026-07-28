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
  serialize,
  deserialize,
  _resetIdCounter,
} from "../split-tree.svelte";

beforeEach(() => {
  resetTree();
  _resetIdCounter();
});

describe("initTree", () => {
  it("creates a single leaf with given sessions", () => {
    initTree(["s1", "s2"]);
    const tree = getTree();
    expect(tree).not.toBeNull();
    expect(tree!.type).toBe("leaf");
    if (tree!.type === "leaf") {
      expect(tree!.tabs).toEqual(["s1", "s2"]);
      expect(tree!.activeTab).toBe(0);
    }
    expect(getFocusedLeafId()).toBe(tree!.id);
  });

  it("respects activeIndex parameter", () => {
    initTree(["s1", "s2", "s3"], 2);
    const leaf = getFocusedLeaf();
    expect(leaf!.activeTab).toBe(2);
  });
});

describe("splitFocusedLeaf", () => {
  it("splits into a binary tree with original tabs in first child", () => {
    initTree(["s1", "s2"]);
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

      const first = tree!.children[0] as { type: "leaf"; id: string; tabs: string[] };
      const second = tree!.children[1] as { type: "leaf"; id: string; tabs: string[] };
      expect(first.tabs).toEqual(["s1", "s2"]);
      expect(first.id).toBe(originalLeafId);
      expect(second.tabs).toEqual([]);
      expect(second.id).toBe(newLeafId);
    }
  });

  it("moves focus to the new leaf", () => {
    initTree(["s1"]);
    const newLeafId = splitFocusedLeaf("horizontal");
    expect(getFocusedLeafId()).toBe(newLeafId);
  });

  it("supports nested splits (recursive binary tree)", () => {
    initTree(["s1"]);
    splitFocusedLeaf("vertical"); // new leaf is focused
    addSessionToLeaf(getFocusedLeafId()!, "s2");
    splitFocusedLeaf("horizontal"); // split the new leaf again

    const leaves = getAllLeaves();
    expect(leaves.length).toBe(3);
  });

  it("returns null when no tree exists", () => {
    expect(splitFocusedLeaf("vertical")).toBeNull();
  });
});

describe("addSessionToLeaf", () => {
  it("appends session and updates activeTab", () => {
    initTree(["s1"]);
    const leafId = getFocusedLeafId()!;
    addSessionToLeaf(leafId, "s2");

    const leaf = getFocusedLeaf()!;
    expect(leaf.tabs).toEqual(["s1", "s2"]);
    expect(leaf.activeTab).toBe(1);
  });
});

describe("removeSessionFromLeaf", () => {
  it("removes session and keeps leaf alive if tabs remain", () => {
    initTree(["s1", "s2"]);
    const destroyed = removeSessionFromLeaf("s1");
    expect(destroyed).toBe(false);
    const leaf = getFocusedLeaf()!;
    expect(leaf.tabs).toEqual(["s2"]);
  });

  it("destroys leaf when last tab is removed", () => {
    initTree(["s1"]);
    splitFocusedLeaf("vertical");
    const emptyLeafId = getFocusedLeafId()!;
    addSessionToLeaf(emptyLeafId, "s2");

    // Now we have: split -> [leaf(s1), leaf(s2)]
    // Remove s2 — leaf should be destroyed
    setFocusedLeaf(emptyLeafId);
    const destroyed = removeSessionFromLeaf("s2");
    expect(destroyed).toBe(true);

    // Tree should collapse to single leaf
    const tree = getTree();
    expect(tree!.type).toBe("leaf");
  });
});

describe("moveSessionToLeaf", () => {
  it("moves a session from one leaf to another", () => {
    initTree(["s1", "s2"]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, "s3");

    moveSessionToLeaf("s1", newLeafId);

    const leaves = getAllLeaves();
    const original = leaves.find((l) => l.id === originalLeafId)!;
    const target = leaves.find((l) => l.id === newLeafId)!;
    expect(original.tabs).toEqual(["s2"]);
    expect(target.tabs).toContain("s1");
    expect(target.tabs).toContain("s3");
  });

  it("moves at specific insertIndex", () => {
    initTree(["s1", "s2"]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, "s3");
    addSessionToLeaf(newLeafId, "s4");

    moveSessionToLeaf("s1", newLeafId, 1);

    const target = getAllLeaves().find((l) => l.id === newLeafId)!;
    expect(target.tabs).toEqual(["s3", "s1", "s4"]);
  });

  it("destroys source leaf when last tab is moved out", () => {
    initTree(["s1"]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, "s2");

    moveSessionToLeaf("s1", newLeafId);

    // Original leaf should be destroyed, tree collapses
    const tree = getTree();
    expect(tree!.type).toBe("leaf");
    if (tree!.type === "leaf") {
      expect(tree!.tabs).toContain("s1");
      expect(tree!.tabs).toContain("s2");
    }
  });
});

describe("closeSplit", () => {
  it("migrates tabs to sibling and destroys the split", () => {
    initTree(["s1"]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;
    addSessionToLeaf(newLeafId, "s2");
    addSessionToLeaf(newLeafId, "s3");

    // Close the new leaf's split — tabs migrate to sibling (original)
    closeSplit(newLeafId);

    const tree = getTree();
    expect(tree!.type).toBe("leaf");
    if (tree!.type === "leaf") {
      expect(tree!.tabs).toEqual(["s1", "s2", "s3"]);
    }
  });

  it("does nothing on root leaf", () => {
    initTree(["s1"]);
    closeSplit(getFocusedLeafId()!);
    expect(getTree()!.type).toBe("leaf");
  });
});

describe("destroyLeaf", () => {
  it("collapses tree when leaf is destroyed", () => {
    initTree(["s1"]);
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;

    destroyLeaf(newLeafId);

    const tree = getTree();
    expect(tree!.type).toBe("leaf");
  });

  it("clears tree when last leaf is destroyed", () => {
    initTree(["s1"]);
    destroyLeaf(getFocusedLeafId()!);
    expect(getTree()).toBeNull();
    expect(getFocusedLeafId()).toBeNull();
  });

  it("updates focus to sibling", () => {
    initTree(["s1"]);
    const originalLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const newLeafId = getFocusedLeafId()!;

    destroyLeaf(newLeafId);
    expect(getFocusedLeafId()).toBe(originalLeafId);
  });
});

describe("setLeafActiveTab", () => {
  it("sets active tab index", () => {
    initTree(["s1", "s2", "s3"]);
    setLeafActiveTab(getFocusedLeafId()!, 2);
    expect(getFocusedLeaf()!.activeTab).toBe(2);
  });

  it("clamps to valid range", () => {
    initTree(["s1", "s2"]);
    setLeafActiveTab(getFocusedLeafId()!, 10);
    expect(getFocusedLeaf()!.activeTab).toBe(1);
  });
});

describe("setRatio", () => {
  it("updates the split ratio", () => {
    initTree(["s1"]);
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
    initTree(["s1"]);
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
    initTree(["s1"]);
    const leftLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical"); // new leaf is right, now focused

    setFocusedLeaf(leftLeafId);
    const neighbor = getNeighborLeaf("right");
    expect(neighbor).not.toBeNull();
    expect(neighbor!.tabs).toEqual([]);
  });

  it("finds left neighbor in vertical split", () => {
    initTree(["s1"]);
    splitFocusedLeaf("vertical"); // focused is right (empty)

    const neighbor = getNeighborLeaf("left");
    expect(neighbor).not.toBeNull();
    expect(neighbor!.tabs).toEqual(["s1"]);
  });

  it("finds down neighbor in horizontal split", () => {
    initTree(["s1"]);
    const topLeafId = getFocusedLeafId()!;
    splitFocusedLeaf("horizontal"); // new leaf is bottom

    setFocusedLeaf(topLeafId);
    const neighbor = getNeighborLeaf("down");
    expect(neighbor).not.toBeNull();
  });

  it("returns null when no neighbor exists", () => {
    initTree(["s1"]);
    expect(getNeighborLeaf("left")).toBeNull();
    expect(getNeighborLeaf("right")).toBeNull();
  });

  it("focusDirection changes focused leaf", () => {
    initTree(["s1"]);
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
    initTree(["s1", "s2"]);
    const leftId = getFocusedLeafId()!;
    splitFocusedLeaf("vertical");
    const rightId = getFocusedLeafId()!;
    addSessionToLeaf(rightId, "s3");

    setFocusedLeaf(leftId);
    setLeafActiveTab(leftId, 0); // s1 is active

    moveTabToDirection("right");

    const rightLeaf = getAllLeaves().find((l) => l.id === rightId);
    expect(rightLeaf!.tabs).toContain("s1");
  });

  it("creates a new split when no neighbor exists", () => {
    initTree(["s1", "s2"]);
    setLeafActiveTab(getFocusedLeafId()!, 0);

    const result = moveTabToDirection("right");
    expect(result).not.toBeNull();

    const leaves = getAllLeaves();
    expect(leaves.length).toBe(2);
    // s1 should be in the new leaf (right)
    const newLeaf = leaves.find((l) => l.tabs.includes("s1"));
    expect(newLeaf).not.toBeNull();
    // s2 remains in the original
    const originalLeaf = leaves.find((l) => l.tabs.includes("s2"));
    expect(originalLeaf).not.toBeNull();
  });
});

describe("getLeafForSession", () => {
  it("finds the leaf containing a session", () => {
    initTree(["s1", "s2"]);
    const leaf = getLeafForSession("s1");
    expect(leaf).not.toBeNull();
    expect(leaf!.tabs).toContain("s1");
  });

  it("returns null for unknown session", () => {
    initTree(["s1"]);
    expect(getLeafForSession("unknown")).toBeNull();
  });
});

describe("serialize / deserialize", () => {
  it("round-trips the tree", () => {
    initTree(["s1", "s2"]);
    splitFocusedLeaf("vertical");
    addSessionToLeaf(getFocusedLeafId()!, "s3");

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
