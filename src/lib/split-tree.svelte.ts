/**
 * Split Tree — binary tree model for pane splitting.
 *
 * Topology: strict binary tree. Internal nodes are splits (horizontal/vertical)
 * with exactly two children. Leaf nodes hold an ordered list of session IDs (tabs).
 *
 * Key behaviors:
 * - Splitting a focused leaf creates a new child split with the original tabs and an empty new leaf.
 * - The new leaf opens a raw terminal (handled by the UI layer via the `onNewLeafCreated` callback).
 * - Closing the last tab in a leaf destroys the leaf; sibling expands.
 * - Closing a split ("X" button) migrates tabs to the sibling, then destroys the split.
 * - Focus navigation is spatial (visual neighbor).
 * - Resize ratios persist.
 */

// ─── Types ───────────────────────────────────────────────────────────────────

export type SplitDirection = "horizontal" | "vertical";

export interface SplitNode {
  type: "split";
  id: string;
  direction: SplitDirection;
  ratio: number; // 0–1, size of first child relative to total
  children: [TreeNode, TreeNode];
}

export interface LeafNode {
  type: "leaf";
  id: string;
  tabs: string[]; // session IDs
  activeTab: number; // index into tabs[]
}

export type TreeNode = SplitNode | LeafNode;

export interface SerializedTree {
  tree: TreeNode;
  focusedLeafId: string;
}

// ─── ID generation ───────────────────────────────────────────────────────────

let nextId = 0;
function generateId(): string {
  return `split_${Date.now()}_${nextId++}`;
}

/** Reset ID counter (for tests) */
export function _resetIdCounter(): void {
  nextId = 0;
}

// ─── State ───────────────────────────────────────────────────────────────────

let tree = $state<TreeNode | null>(null);
let focusedLeafId = $state<string | null>(null);

// ─── Getters ─────────────────────────────────────────────────────────────────

export function getTree(): TreeNode | null {
  return tree;
}

export function getFocusedLeafId(): string | null {
  return focusedLeafId;
}

export function getFocusedLeaf(): LeafNode | null {
  if (!tree || !focusedLeafId) return null;
  return findLeaf(tree, focusedLeafId);
}

export function getLeafForSession(sessionId: string): LeafNode | null {
  if (!tree) return null;
  return findLeafBySession(tree, sessionId);
}

export function getAllLeaves(): LeafNode[] {
  if (!tree) return [];
  return collectLeaves(tree);
}

export function getLeafById(id: string): LeafNode | null {
  if (!tree) return null;
  return findLeaf(tree, id);
}

// ─── Initialization ──────────────────────────────────────────────────────────

/** Initialize with a single leaf containing the given session IDs. */
export function initTree(sessionIds: string[], activeIndex = 0): void {
  const leafId = generateId();
  tree = { type: "leaf", id: leafId, tabs: [...sessionIds], activeTab: activeIndex };
  focusedLeafId = leafId;
}

/** Replace the entire tree (used for deserialization). */
export function setTree(newTree: TreeNode, newFocusedLeafId: string): void {
  tree = newTree;
  focusedLeafId = newFocusedLeafId;
}

/** Reset all state (for tests or cleanup). */
export function resetTree(): void {
  tree = null;
  focusedLeafId = null;
}

// ─── Split Operations ────────────────────────────────────────────────────────

/**
 * Split the focused leaf in the given direction.
 * Returns the ID of the new (empty) leaf, or null if no focused leaf.
 */
export function splitFocusedLeaf(direction: SplitDirection): string | null {
  if (!tree || !focusedLeafId) return null;
  const newLeafId = generateId();
  const splitId = generateId();

  tree = mapTree(tree, (node) => {
    if (node.type === "leaf" && node.id === focusedLeafId) {
      // The focused leaf becomes the first child; new empty leaf is the second.
      const split: SplitNode = {
        type: "split",
        id: splitId,
        direction,
        ratio: 0.5,
        children: [
          { ...node }, // original leaf keeps its tabs
          { type: "leaf", id: newLeafId, tabs: [], activeTab: 0 },
        ],
      };
      return split;
    }
    return node;
  });

  // Focus moves to the new leaf
  focusedLeafId = newLeafId;
  return newLeafId;
}

/**
 * Add a session to a specific leaf.
 */
export function addSessionToLeaf(leafId: string, sessionId: string): void {
  if (!tree) return;
  tree = mapTree(tree, (node) => {
    if (node.type === "leaf" && node.id === leafId) {
      return { ...node, tabs: [...node.tabs, sessionId], activeTab: node.tabs.length };
    }
    return node;
  });
}

/**
 * Remove a session from its leaf. If the leaf becomes empty, destroy it.
 * Returns true if the leaf was destroyed.
 */
export function removeSessionFromLeaf(sessionId: string): boolean {
  if (!tree) return false;
  const leaf = findLeafBySession(tree, sessionId);
  if (!leaf) return false;

  const remainingTabs = leaf.tabs.filter((id) => id !== sessionId);
  if (remainingTabs.length === 0) {
    // Leaf is now empty — destroy it
    destroyLeaf(leaf.id);
    return true;
  }

  // Update the leaf in place
  tree = mapTree(tree, (node) => {
    if (node.type === "leaf" && node.id === leaf.id) {
      const newActive = Math.min(node.activeTab, remainingTabs.length - 1);
      return { ...node, tabs: remainingTabs, activeTab: newActive };
    }
    return node;
  });
  return false;
}

/**
 * Move a session from its current leaf to a target leaf at a specific index.
 */
export function moveSessionToLeaf(
  sessionId: string,
  targetLeafId: string,
  insertIndex?: number,
): void {
  if (!tree) return;
  const sourceLeaf = findLeafBySession(tree, sessionId);
  if (!sourceLeaf || sourceLeaf.id === targetLeafId) {
    // Same leaf — just reorder
    if (sourceLeaf && insertIndex !== undefined) {
      tree = mapTree(tree, (node) => {
        if (node.type === "leaf" && node.id === sourceLeaf.id) {
          const tabs = node.tabs.filter((id) => id !== sessionId);
          const idx = Math.min(insertIndex, tabs.length);
          tabs.splice(idx, 0, sessionId);
          return { ...node, tabs, activeTab: idx };
        }
        return node;
      });
    }
    return;
  }

  // Remove from source
  const sourceTabs = sourceLeaf.tabs.filter((id) => id !== sessionId);
  const sourceDestroyed = sourceTabs.length === 0;

  // Add to target
  tree = mapTree(tree, (node) => {
    if (node.type === "leaf" && node.id === targetLeafId) {
      const idx = insertIndex !== undefined ? Math.min(insertIndex, node.tabs.length) : node.tabs.length;
      const newTabs = [...node.tabs];
      newTabs.splice(idx, 0, sessionId);
      return { ...node, tabs: newTabs, activeTab: idx };
    }
    if (node.type === "leaf" && node.id === sourceLeaf.id) {
      if (sourceDestroyed) return node; // will be cleaned up below
      const newActive = Math.min(node.activeTab, sourceTabs.length - 1);
      return { ...node, tabs: sourceTabs, activeTab: newActive };
    }
    return node;
  });

  if (sourceDestroyed) {
    destroyLeaf(sourceLeaf.id);
  }
}

/**
 * Close a split: migrate all tabs from the target leaf to its sibling,
 * then destroy the parent split.
 */
export function closeSplit(leafId: string): void {
  if (!tree) return;
  if (tree.type === "leaf") return; // can't close the root leaf

  const parent = findParentSplit(tree, leafId);
  if (!parent) return;

  const [child0, child1] = parent.children;
  const isFirst = containsLeaf(child0, leafId);
  const closingChild = isFirst ? child0 : child1;
  const siblingChild = isFirst ? child1 : child0;

  // Collect all tabs from the closing subtree
  const closingLeaves = collectLeaves(closingChild);
  const allTabs = closingLeaves.flatMap((l) => l.tabs);

  // Find the first leaf of the sibling to receive tabs
  const siblingLeaves = collectLeaves(siblingChild);
  const receiverLeaf = siblingLeaves[0];

  // Replace the parent split with the sibling subtree (with migrated tabs)
  let updatedSibling = siblingChild;
  if (receiverLeaf && allTabs.length > 0) {
    updatedSibling = mapTree(siblingChild, (node) => {
      if (node.type === "leaf" && node.id === receiverLeaf.id) {
        return { ...node, tabs: [...node.tabs, ...allTabs] };
      }
      return node;
    });
  }

  tree = replaceNode(tree, parent.id, updatedSibling);

  // Update focus
  if (focusedLeafId && containsLeafId(closingChild, focusedLeafId)) {
    focusedLeafId = receiverLeaf?.id ?? collectLeaves(tree!)[0]?.id ?? null;
  }
}

/**
 * Destroy a leaf (used when last tab is closed).
 * Sibling takes over the parent split's position.
 */
export function destroyLeaf(leafId: string): void {
  if (!tree) return;
  if (tree.type === "leaf" && tree.id === leafId) {
    // Last leaf — just clear
    tree = null;
    focusedLeafId = null;
    return;
  }

  const parent = findParentSplit(tree, leafId);
  if (!parent) return;

  const [child0, child1] = parent.children;
  const isFirst = child0.type === "leaf" && child0.id === leafId;
  const sibling = isFirst ? child1 : child0;

  tree = replaceNode(tree, parent.id, sibling);

  // Update focus to sibling's first leaf
  if (focusedLeafId === leafId) {
    const sibLeaves = collectLeaves(sibling);
    focusedLeafId = sibLeaves[0]?.id ?? null;
  }
}

// ─── Focus Management ────────────────────────────────────────────────────────

export function setFocusedLeaf(leafId: string): void {
  focusedLeafId = leafId;
}

/**
 * Set the active tab index on a leaf.
 */
export function setLeafActiveTab(leafId: string, tabIndex: number): void {
  if (!tree) return;
  tree = mapTree(tree, (node) => {
    if (node.type === "leaf" && node.id === leafId) {
      return { ...node, activeTab: Math.max(0, Math.min(tabIndex, node.tabs.length - 1)) };
    }
    return node;
  });
}

// ─── Resize ──────────────────────────────────────────────────────────────────

export function setRatio(splitId: string, ratio: number): void {
  if (!tree) return;
  const clamped = Math.max(0.1, Math.min(0.9, ratio));
  tree = mapTree(tree, (node) => {
    if (node.type === "split" && node.id === splitId) {
      return { ...node, ratio: clamped };
    }
    return node;
  });
}

// ─── Spatial Navigation ──────────────────────────────────────────────────────

export type NavDirection = "left" | "right" | "up" | "down";

/**
 * Find the neighbor leaf in the given direction from the focused leaf.
 * Uses the tree structure to compute spatial relationships.
 */
export function getNeighborLeaf(direction: NavDirection): LeafNode | null {
  if (!tree || !focusedLeafId) return null;
  const leaves = collectLeavesWithBounds(tree, { x: 0, y: 0, w: 1, h: 1 });
  const current = leaves.find((l) => l.leaf.id === focusedLeafId);
  if (!current) return null;

  const { x, y, w, h } = current.bounds;
  const cx = x + w / 2;
  const cy = y + h / 2;

  // Filter candidates in the right direction
  const candidates = leaves.filter((l) => {
    if (l.leaf.id === focusedLeafId) return false;
    const lx = l.bounds.x + l.bounds.w / 2;
    const ly = l.bounds.y + l.bounds.h / 2;
    switch (direction) {
      case "left": return lx < cx;
      case "right": return lx > cx;
      case "up": return ly < cy;
      case "down": return ly > cy;
    }
  });

  if (candidates.length === 0) return null;

  // Pick the closest by center distance
  candidates.sort((a, b) => {
    const ax = a.bounds.x + a.bounds.w / 2;
    const ay = a.bounds.y + a.bounds.h / 2;
    const bx = b.bounds.x + b.bounds.w / 2;
    const by = b.bounds.y + b.bounds.h / 2;
    const da = Math.abs(ax - cx) + Math.abs(ay - cy);
    const db = Math.abs(bx - cx) + Math.abs(by - cy);
    return da - db;
  });

  return candidates[0].leaf;
}

/**
 * Navigate focus to the neighbor in the given direction.
 * Returns the new focused leaf ID, or null if no neighbor.
 */
export function focusDirection(direction: NavDirection): string | null {
  const neighbor = getNeighborLeaf(direction);
  if (neighbor) {
    focusedLeafId = neighbor.id;
    return neighbor.id;
  }
  return null;
}

/**
 * Move the active tab of the focused leaf to the neighbor in the given direction.
 * If no neighbor exists, create a split in that direction.
 * Returns the target leaf ID.
 */
export function moveTabToDirection(direction: NavDirection): string | null {
  if (!tree || !focusedLeafId) return null;
  const focusedLeaf = findLeaf(tree, focusedLeafId);
  if (!focusedLeaf || focusedLeaf.tabs.length === 0) return null;

  const sessionId = focusedLeaf.tabs[focusedLeaf.activeTab];
  if (!sessionId) return null;

  const neighbor = getNeighborLeaf(direction);
  if (neighbor) {
    moveSessionToLeaf(sessionId, neighbor.id);
    return neighbor.id;
  }

  // No neighbor — create a new split
  const splitDirection: SplitDirection =
    direction === "left" || direction === "right" ? "vertical" : "horizontal";

  // We need to split the focused leaf, but place the new leaf on the correct side
  const newLeafId = generateId();
  const splitId = generateId();

  const isNewFirst = direction === "left" || direction === "up";

  tree = mapTree(tree, (node) => {
    if (node.type === "leaf" && node.id === focusedLeafId) {
      const originalLeaf: LeafNode = { ...node, tabs: node.tabs.filter((id) => id !== sessionId), activeTab: Math.min(node.activeTab, Math.max(0, node.tabs.length - 2)) };
      const newLeaf: LeafNode = { type: "leaf", id: newLeafId, tabs: [sessionId], activeTab: 0 };
      const split: SplitNode = {
        type: "split",
        id: splitId,
        direction: splitDirection,
        children: isNewFirst ? [newLeaf, originalLeaf] : [originalLeaf, newLeaf],
        ratio: 0.5,
      };
      return split;
    }
    return node;
  });

  // If the original leaf is now empty, we need to clean it up
  const originalLeafAfter = findLeaf(tree!, focusedLeafId!);
  if (originalLeafAfter && originalLeafAfter.tabs.length === 0) {
    destroyLeaf(originalLeafAfter.id);
  }

  focusedLeafId = newLeafId;
  return newLeafId;
}

// ─── Serialization ───────────────────────────────────────────────────────────

export function serialize(): SerializedTree | null {
  if (!tree || !focusedLeafId) return null;
  return { tree: structuredClone($state.snapshot(tree)), focusedLeafId };
}

export function deserialize(data: SerializedTree): void {
  tree = data.tree;
  focusedLeafId = data.focusedLeafId;
}

// ─── Tree Helpers (pure functions) ───────────────────────────────────────────

function findLeaf(node: TreeNode, leafId: string): LeafNode | null {
  if (node.type === "leaf") return node.id === leafId ? node : null;
  return findLeaf(node.children[0], leafId) ?? findLeaf(node.children[1], leafId);
}

function findLeafBySession(node: TreeNode, sessionId: string): LeafNode | null {
  if (node.type === "leaf") return node.tabs.includes(sessionId) ? node : null;
  return findLeafBySession(node.children[0], sessionId) ?? findLeafBySession(node.children[1], sessionId);
}

function collectLeaves(node: TreeNode): LeafNode[] {
  if (node.type === "leaf") return [node];
  return [...collectLeaves(node.children[0]), ...collectLeaves(node.children[1])];
}

function containsLeaf(node: TreeNode, leafId: string): boolean {
  if (node.type === "leaf") return node.id === leafId;
  return containsLeaf(node.children[0], leafId) || containsLeaf(node.children[1], leafId);
}

function containsLeafId(node: TreeNode, leafId: string): boolean {
  return containsLeaf(node, leafId);
}

function findParentSplit(node: TreeNode, childId: string): SplitNode | null {
  if (node.type === "leaf") return null;
  const [c0, c1] = node.children;
  // Direct child match
  if (c0.id === childId || c1.id === childId) return node;
  // Recurse into children
  return findParentSplit(c0, childId) ?? findParentSplit(c1, childId);
}

/**
 * Map over the tree, replacing nodes. Returns a new tree.
 * The mapper is called bottom-up.
 */
function mapTree(node: TreeNode, fn: (node: TreeNode) => TreeNode): TreeNode {
  if (node.type === "leaf") return fn(node);
  const mapped: SplitNode = {
    ...node,
    children: [mapTree(node.children[0], fn) as TreeNode, mapTree(node.children[1], fn) as TreeNode] as [TreeNode, TreeNode],
  };
  return fn(mapped);
}

/** Replace a node by ID with a replacement subtree. */
function replaceNode(root: TreeNode, nodeId: string, replacement: TreeNode): TreeNode {
  if (root.id === nodeId) return replacement;
  if (root.type === "leaf") return root;
  return {
    ...root,
    children: [
      replaceNode(root.children[0], nodeId, replacement),
      replaceNode(root.children[1], nodeId, replacement),
    ] as [TreeNode, TreeNode],
  };
}

interface Bounds {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface LeafWithBounds {
  leaf: LeafNode;
  bounds: Bounds;
}

function collectLeavesWithBounds(node: TreeNode, bounds: Bounds): LeafWithBounds[] {
  if (node.type === "leaf") return [{ leaf: node, bounds }];
  const { direction, ratio } = node;
  const [c0, c1] = node.children;

  let bounds0: Bounds;
  let bounds1: Bounds;

  if (direction === "vertical") {
    bounds0 = { x: bounds.x, y: bounds.y, w: bounds.w * ratio, h: bounds.h };
    bounds1 = { x: bounds.x + bounds.w * ratio, y: bounds.y, w: bounds.w * (1 - ratio), h: bounds.h };
  } else {
    bounds0 = { x: bounds.x, y: bounds.y, w: bounds.w, h: bounds.h * ratio };
    bounds1 = { x: bounds.x, y: bounds.y + bounds.h * ratio, w: bounds.w, h: bounds.h * (1 - ratio) };
  }

  return [...collectLeavesWithBounds(c0, bounds0), ...collectLeavesWithBounds(c1, bounds1)];
}
