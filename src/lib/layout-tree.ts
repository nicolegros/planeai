/**
 * Layout tree: the pure value behind a TaskWorkspace's tabs and splits.
 *
 * A strict binary tree: splits have exactly two children, leaves (panes) hold an
 * ordered list of tabs keyed by pty key. Every operation returns a new layout and
 * returns its input unchanged when nothing changed, so a reactive holder only
 * notifies on real edits.
 *
 * Invariant: a pty key appears at most once in the whole tree. Tabs render in a
 * keyed block, so a duplicate would crash the workspace; every entry point that
 * accepts tabs (add, create, restore) enforces it.
 */
import { parsePtyKey, ptyKeySessionId } from "./pty-key";

export type SplitDirection = "horizontal" | "vertical";
export type NavDirection = "left" | "right" | "up" | "down";
export type TabType = "agent" | "shell" | "diff" | "editor";

export interface TabEntry {
  ptyKey: string;
  label: string;
  icon: string;
  type: TabType;
  /** The label was set by the user or the shell and must survive relabeling. */
  customTitle?: boolean;
  filePath?: string;
}

export interface SplitNode {
  type: "split";
  id: string;
  direction: SplitDirection;
  /** Size of the first child relative to the whole, in [0.1, 0.9]. */
  ratio: number;
  children: [TreeNode, TreeNode];
}

export interface LeafNode {
  type: "leaf";
  id: string;
  tabs: TabEntry[];
  /** Pty key of the tab in front; may be stale or empty, see `activeTabOf`. */
  activeTab: string;
}

export type TreeNode = SplitNode | LeafNode;

/** Also the persisted format (`layout_json`); keep field names stable. */
export interface Layout {
  tree: TreeNode;
  focusedLeafId: string;
}

const MAX_DEPTH = 32;

const newId = (): string => crypto.randomUUID();

// ─── Queries ─────────────────────────────────────────────────────────────────

export function leavesOf(layout: Layout | null): LeafNode[] {
  return layout ? collectLeaves(layout.tree) : [];
}

export function findLeaf(layout: Layout | null, leafId: string): LeafNode | null {
  return leavesOf(layout).find((leaf) => leaf.id === leafId) ?? null;
}

export function findTab(
  layout: Layout | null,
  ptyKey: string,
): { leaf: LeafNode; tab: TabEntry } | null {
  for (const leaf of leavesOf(layout)) {
    const tab = leaf.tabs.find((candidate) => candidate.ptyKey === ptyKey);
    if (tab) return { leaf, tab };
  }
  return null;
}

export function focusedLeafOf(layout: Layout | null): LeafNode | null {
  return layout ? findLeaf(layout, layout.focusedLeafId) : null;
}

/** The tab in front of a pane, falling back to its first tab. */
export function activeTabOf(leaf: LeafNode | null): TabEntry | null {
  if (!leaf || leaf.tabs.length === 0) return null;
  return leaf.tabs.find((tab) => tab.ptyKey === leaf.activeTab) ?? leaf.tabs[0];
}

/** The tab in front of the focused pane. */
export function focusedTabOf(layout: Layout | null): TabEntry | null {
  return activeTabOf(focusedLeafOf(layout));
}

export function tabsOf(layout: Layout | null): TabEntry[] {
  return leavesOf(layout).flatMap((leaf) => leaf.tabs);
}

export function isSplit(layout: Layout | null): boolean {
  return layout?.tree.type === "split";
}

// ─── Construction ────────────────────────────────────────────────────────────

/** A single pane holding `tabs`; repeated pty keys are dropped. */
export function createLayout(tabs: TabEntry[], activeTab?: string): Layout {
  const unique = retainUnseen(tabs, new Set());
  const leaf: LeafNode = {
    type: "leaf",
    id: newId(),
    tabs: unique,
    activeTab: activeTab ?? unique[0]?.ptyKey ?? "",
  };
  return { tree: leaf, focusedLeafId: leaf.id };
}

// ─── Tabs ────────────────────────────────────────────────────────────────────

/**
 * Add a tab to a pane and bring it to the front. A pty key already in the tree
 * is updated in place and activated in whichever pane holds it instead, so the
 * key never appears twice. Unchanged when `leafId` is not in the tree.
 */
export function addTab(layout: Layout, leafId: string, tab: TabEntry): Layout {
  const existing = findTab(layout, tab.ptyKey);
  if (existing) {
    return updateLeaf(layout, existing.leaf.id, (leaf) => ({
      ...leaf,
      tabs: leaf.tabs.map((candidate) => (candidate.ptyKey === tab.ptyKey ? tab : candidate)),
      activeTab: tab.ptyKey,
    }));
  }
  return updateLeaf(layout, leafId, (leaf) => ({
    ...leaf,
    tabs: [...leaf.tabs, tab],
    activeTab: tab.ptyKey,
  }));
}

/** Remove a tab; a pane left empty is destroyed, and so is the layout with its last pane. */
export function removeTab(layout: Layout, ptyKey: string): Layout | null {
  const found = findTab(layout, ptyKey);
  if (!found) return layout;
  const remaining = found.leaf.tabs.filter((tab) => tab.ptyKey !== ptyKey);
  if (remaining.length === 0) return destroyLeaf(layout, found.leaf.id);
  return updateLeaf(layout, found.leaf.id, (leaf) => ({
    ...leaf,
    tabs: remaining,
    activeTab: nextActive(leaf, remaining, ptyKey),
  }));
}

/** Bring a tab to the front of its pane and focus that pane. */
export function focusTab(layout: Layout, ptyKey: string): Layout {
  const found = findTab(layout, ptyKey);
  if (!found) return layout;
  const updated = updateLeaf(layout, found.leaf.id, (leaf) =>
    leaf.activeTab === ptyKey ? leaf : { ...leaf, activeTab: ptyKey },
  );
  return focusLeaf(updated, found.leaf.id);
}

/** Move the focused pane's front tab `delta` places, wrapping around. */
export function cycleTab(layout: Layout, delta: number): Layout {
  const leaf = focusedLeafOf(layout);
  const current = activeTabOf(leaf);
  if (!leaf || !current || leaf.tabs.length <= 1) return layout;
  const index = leaf.tabs.indexOf(current);
  const next =
    leaf.tabs[(((index + delta) % leaf.tabs.length) + leaf.tabs.length) % leaf.tabs.length];
  return focusTab(layout, next.ptyKey);
}

/**
 * Move a tab into a pane at `index` (the end by default), bringing it to the
 * front and focusing that pane. A source pane left empty is destroyed.
 */
export function moveTab(
  layout: Layout,
  ptyKey: string,
  targetLeafId: string,
  index?: number,
): Layout {
  const found = findTab(layout, ptyKey);
  const target = findLeaf(layout, targetLeafId);
  if (!found || !target) return layout;

  if (found.leaf.id === targetLeafId) {
    const tabs = found.leaf.tabs.filter((tab) => tab.ptyKey !== ptyKey);
    const sourceIndex = found.leaf.tabs.indexOf(found.tab);
    // An index past the dragged tab counts the tab itself; drop it from the count.
    const at = Math.min(
      index === undefined ? tabs.length : index > sourceIndex ? index - 1 : index,
      tabs.length,
    );
    tabs.splice(at, 0, found.tab);
    const reordered = updateLeaf(layout, targetLeafId, (leaf) => ({
      ...leaf,
      tabs,
      activeTab: ptyKey,
    }));
    return focusLeaf(reordered, targetLeafId);
  }

  const sourceTabs = found.leaf.tabs.filter((tab) => tab.ptyKey !== ptyKey);
  let moved = updateLeaf(layout, targetLeafId, (leaf) => {
    const tabs = [...leaf.tabs];
    tabs.splice(Math.min(index ?? tabs.length, tabs.length), 0, found.tab);
    return { ...leaf, tabs, activeTab: ptyKey };
  });
  moved = focusLeaf(moved, targetLeafId);
  if (sourceTabs.length === 0) return destroyLeaf(moved, found.leaf.id) ?? moved;
  return updateLeaf(moved, found.leaf.id, (leaf) => ({
    ...leaf,
    tabs: sourceTabs,
    activeTab: nextActive(leaf, sourceTabs, ptyKey),
  }));
}

/** Set a tab's label as a custom title that relabeling keeps. */
export function setTabTitle(layout: Layout, ptyKey: string, label: string): Layout {
  const found = findTab(layout, ptyKey);
  if (!found || (found.tab.label === label && found.tab.customTitle)) return layout;
  return updateLeaf(layout, found.leaf.id, (leaf) => ({
    ...leaf,
    tabs: leaf.tabs.map((tab) =>
      tab.ptyKey === ptyKey ? { ...tab, label, customTitle: true } : tab,
    ),
  }));
}

/** Refresh derived labels (e.g. after a session rename); custom titles are kept. */
export function relabel(layout: Layout, labels: ReadonlyMap<string, string>): Layout {
  const stale = (tab: TabEntry) =>
    !tab.customTitle && labels.has(tab.ptyKey) && labels.get(tab.ptyKey) !== tab.label;
  let result = layout;
  for (const leaf of leavesOf(layout)) {
    if (!leaf.tabs.some(stale)) continue;
    result = updateLeaf(result, leaf.id, (current) => ({
      ...current,
      tabs: current.tabs.map((tab) =>
        stale(tab) ? { ...tab, label: labels.get(tab.ptyKey)! } : tab,
      ),
    }));
  }
  return result;
}

// ─── Panes ───────────────────────────────────────────────────────────────────

export function focusLeaf(layout: Layout, leafId: string): Layout {
  if (layout.focusedLeafId === leafId || !findLeaf(layout, leafId)) return layout;
  return { ...layout, focusedLeafId: leafId };
}

/**
 * Split the focused pane: it keeps its tabs as the first child, and a new empty
 * pane, which takes focus, becomes the second. Null at the depth limit.
 */
export function splitFocused(
  layout: Layout,
  direction: SplitDirection,
): { layout: Layout; leafId: string } | null {
  if (treeDepth(layout.tree) >= MAX_DEPTH || !focusedLeafOf(layout)) return null;
  const leafId = newId();
  const tree = replaceNode(layout.tree, layout.focusedLeafId, (node) => ({
    type: "split",
    id: newId(),
    direction,
    ratio: 0.5,
    children: [node, { type: "leaf", id: leafId, tabs: [], activeTab: "" }],
  }));
  return { layout: { tree, focusedLeafId: leafId }, leafId };
}

/**
 * Close a pane, migrating its tabs to the first pane of its sibling subtree.
 * The only pane cannot be closed.
 */
export function closePane(layout: Layout, leafId: string): Layout {
  const parent = findParentSplit(layout.tree, leafId);
  if (!parent) return layout;
  const closingIsFirst = containsLeaf(parent.children[0], leafId);
  const closing = parent.children[closingIsFirst ? 0 : 1];
  const sibling = parent.children[closingIsFirst ? 1 : 0];
  const receiver = collectLeaves(sibling)[0];
  const migrated = collectLeaves(closing).flatMap((leaf) => leaf.tabs);
  const updatedSibling =
    migrated.length === 0
      ? sibling
      : replaceNode(sibling, receiver.id, (node) => ({
          ...(node as LeafNode),
          tabs: [...(node as LeafNode).tabs, ...migrated],
        }));
  const tree = replaceNode(layout.tree, parent.id, () => updatedSibling);
  const focusedLeafId = containsLeaf(closing, layout.focusedLeafId)
    ? receiver.id
    : layout.focusedLeafId;
  return { tree, focusedLeafId };
}

/** Remove a pane and its tabs; its sibling takes the parent's place. */
export function destroyLeaf(layout: Layout, leafId: string): Layout | null {
  if (layout.tree.type === "leaf") return layout.tree.id === leafId ? null : layout;
  const parent = findParentSplit(layout.tree, leafId);
  if (!parent) return layout;
  const sibling = parent.children[containsLeaf(parent.children[0], leafId) ? 1 : 0];
  const tree = replaceNode(layout.tree, parent.id, () => sibling);
  const focusedLeafId =
    layout.focusedLeafId === leafId ? collectLeaves(sibling)[0].id : layout.focusedLeafId;
  return { tree, focusedLeafId };
}

export function setRatio(layout: Layout, splitId: string, ratio: number): Layout {
  const clamped = Math.max(0.1, Math.min(0.9, ratio));
  const tree = replaceNode(layout.tree, splitId, (node) =>
    node.type === "split" && node.ratio !== clamped ? { ...node, ratio: clamped } : node,
  );
  return tree === layout.tree ? layout : { ...layout, tree };
}

// ─── Spatial navigation ──────────────────────────────────────────────────────

/** The closest pane in a direction from the focused one, by visual position. */
export function neighborLeaf(layout: Layout, direction: NavDirection): LeafNode | null {
  const placed = placeLeaves(layout.tree, { x: 0, y: 0, w: 1, h: 1 });
  const current = placed.find((entry) => entry.leaf.id === layout.focusedLeafId);
  if (!current) return null;
  const [cx, cy] = center(current.bounds);
  const candidates = placed.filter((entry) => {
    if (entry === current) return false;
    const [x, y] = center(entry.bounds);
    switch (direction) {
      case "left":
        return x < cx;
      case "right":
        return x > cx;
      case "up":
        return y < cy;
      case "down":
        return y > cy;
    }
  });
  const distance = (entry: PlacedLeaf) => {
    const [x, y] = center(entry.bounds);
    return Math.abs(x - cx) + Math.abs(y - cy);
  };
  candidates.sort((a, b) => distance(a) - distance(b));
  return candidates[0]?.leaf ?? null;
}

export function focusDirection(layout: Layout, direction: NavDirection): Layout {
  const neighbor = neighborLeaf(layout, direction);
  return neighbor ? focusLeaf(layout, neighbor.id) : layout;
}

/**
 * Move the focused pane's front tab to its neighbor in a direction, splitting
 * on that side when there is none. Focus follows the tab.
 */
export function moveFocusedTab(layout: Layout, direction: NavDirection): Layout {
  const source = focusedLeafOf(layout);
  const tab = activeTabOf(source);
  if (!source || !tab) return layout;
  const neighbor = neighborLeaf(layout, direction);
  return neighbor
    ? moveTab(layout, tab.ptyKey, neighbor.id)
    : splitWithTab(layout, tab.ptyKey, source.id, direction);
}

/**
 * Split a pane on one side and move a tab into the new pane, which takes focus.
 * Splitting a pane off its own only tab would leave it empty, so that is a no-op.
 */
export function splitWithTab(
  layout: Layout,
  ptyKey: string,
  targetLeafId: string,
  side: NavDirection,
): Layout {
  const found = findTab(layout, ptyKey);
  const target = findLeaf(layout, targetLeafId);
  if (!found || !target || treeDepth(layout.tree) >= MAX_DEPTH) return layout;
  if (found.leaf.id === targetLeafId && target.tabs.length === 1) return layout;

  // The target keeps existing: it is another pane, or keeps its other tabs.
  const without = removeTab(layout, ptyKey)!;
  const created: LeafNode = { type: "leaf", id: newId(), tabs: [found.tab], activeTab: ptyKey };
  const newFirst = side === "left" || side === "up";
  const tree = replaceNode(without.tree, targetLeafId, (node) => ({
    type: "split",
    id: newId(),
    direction: side === "left" || side === "right" ? "vertical" : "horizontal",
    ratio: 0.5,
    children: newFirst ? [created, node] : [node, created],
  }));
  return { tree, focusedLeafId: created.id };
}

// ─── Reconciliation ──────────────────────────────────────────────────────────

export interface ReconcileInput {
  /** One tab per agent session that belongs to the TaskWorkspace. */
  agentTabs: TabEntry[];
  /**
   * Tab the workspace was loaded or last focused for. It wins when it is among
   * the tabs added here, or when the pane has no active tab of its own (a
   * restored layout can carry an empty `activeTab`); otherwise the pane keeps its
   * front tab so unrelated sessions appearing later cannot move the user.
   */
  preferredActiveTab?: string;
}

/**
 * Bring a layout in line with the workspace's sessions: add a tab for each new
 * agent to the focused pane, drop every tab whose session left, and refresh
 * agent labels. The arrangement and front tabs are otherwise left alone.
 */
export function reconcile(layout: Layout | null, input: ReconcileInput): Layout | null {
  const validSessions = new Set(input.agentTabs.map((tab) => ptyKeySessionId(tab.ptyKey)));
  let result = layout;
  for (const tab of tabsOf(layout)) {
    if (result && !validSessions.has(ptyKeySessionId(tab.ptyKey)))
      result = removeTab(result, tab.ptyKey);
  }

  const missing = input.agentTabs.filter((tab) => !findTab(result, tab.ptyKey));
  const preferred = input.preferredActiveTab;
  if (!result && missing.length > 0) {
    result = createLayout(
      missing,
      preferred && missing.some((tab) => tab.ptyKey === preferred) ? preferred : undefined,
    );
  } else if (result && missing.length > 0) {
    const pane = focusedLeafOf(result) ?? leavesOf(result)[0];
    const previousActive = pane.activeTab;
    for (const tab of missing) result = addTab(result, pane.id, tab);
    // The workspace was loaded for the preferred session: keeping the remembered
    // tab active would leave selection and visible tab in disagreement.
    const preferredWasAdded = !!preferred && missing.some((tab) => tab.ptyKey === preferred);
    const active = preferredWasAdded ? preferred : previousActive || preferred;
    if (active && findTab(result, active)?.leaf.id === pane.id) {
      result = updateLeaf(result, pane.id, (leaf) => ({ ...leaf, activeTab: active }));
    }
  }

  return result
    ? relabel(result, new Map(input.agentTabs.map((tab) => [tab.ptyKey, tab.label])))
    : null;
}

// ─── Persistence ─────────────────────────────────────────────────────────────

export function serializeLayout(layout: Layout): string {
  return JSON.stringify(layout);
}

/**
 * Parse a persisted layout, or null when it is unusable. Repairs what older
 * versions could save: tabs without a `type`, a focused pane that no longer
 * exists, and pty keys repeated across the tree.
 */
export function restoreLayout(json: string): Layout | null {
  let data: unknown;
  try {
    data = JSON.parse(json);
  } catch {
    return null;
  }
  if (!isRecord(data) || typeof data.focusedLeafId !== "string") return null;
  const tree = restoreNode(data.tree, 0, new Set());
  if (!tree) return null;
  const leaves = collectLeaves(tree);
  const focusedLeafId = leaves.some((leaf) => leaf.id === data.focusedLeafId)
    ? data.focusedLeafId
    : leaves[0].id;
  return { tree, focusedLeafId };
}

function restoreNode(node: unknown, depth: number, seen: Set<string>): TreeNode | null {
  if (depth > MAX_DEPTH || !isRecord(node) || typeof node.id !== "string") return null;
  if (node.type === "leaf") {
    if (!Array.isArray(node.tabs) || typeof node.activeTab !== "string") return null;
    const tabs = retainUnseen(
      node.tabs.map(restoreTab).filter((tab) => tab !== null),
      seen,
    );
    const activeTab = tabs.some((tab) => tab.ptyKey === node.activeTab)
      ? node.activeTab
      : (tabs[0]?.ptyKey ?? "");
    return { type: "leaf", id: node.id, tabs, activeTab };
  }
  if (node.type !== "split") return null;
  const { direction, ratio, children } = node;
  if (direction !== "horizontal" && direction !== "vertical") return null;
  if (typeof ratio !== "number" || ratio < 0 || ratio > 1) return null;
  if (!Array.isArray(children) || children.length !== 2) return null;
  const first = restoreNode(children[0], depth + 1, seen);
  const second = first && restoreNode(children[1], depth + 1, seen);
  if (!first || !second) return null;
  return { type: "split", id: node.id, direction, ratio, children: [first, second] };
}

function restoreTab(tab: unknown): TabEntry | null {
  if (!isRecord(tab) || typeof tab.ptyKey !== "string") return null;
  const parts = parsePtyKey(tab.ptyKey);
  if (!parts) return null;
  const type = (tab.type as TabType | undefined) ?? parts.kind;
  return {
    ptyKey: tab.ptyKey,
    label: typeof tab.label === "string" ? tab.label : "",
    icon: typeof tab.icon === "string" ? tab.icon : "terminal",
    type,
    ...(tab.customTitle ? { customTitle: true } : {}),
    ...(typeof tab.filePath === "string"
      ? { filePath: tab.filePath }
      : parts.kind === "editor"
        ? { filePath: parts.filePath }
        : {}),
  };
}

// ─── Tree helpers ────────────────────────────────────────────────────────────

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object";
}

function retainUnseen(tabs: TabEntry[], seen: Set<string>): TabEntry[] {
  return tabs.filter((tab) => {
    if (seen.has(tab.ptyKey)) return false;
    seen.add(tab.ptyKey);
    return true;
  });
}

/** Front tab after removing `removedKey`: the neighbor that slides into its place. */
function nextActive(leaf: LeafNode, remaining: TabEntry[], removedKey: string): string {
  if (leaf.activeTab !== removedKey) return leaf.activeTab;
  const removedIndex = leaf.tabs.findIndex((tab) => tab.ptyKey === removedKey);
  return remaining[Math.min(removedIndex, remaining.length - 1)]?.ptyKey ?? "";
}

function updateLeaf(layout: Layout, leafId: string, update: (leaf: LeafNode) => LeafNode): Layout {
  const tree = replaceNode(layout.tree, leafId, (node) =>
    node.type === "leaf" ? update(node) : node,
  );
  return tree === layout.tree ? layout : { ...layout, tree };
}

/** Replace the node with `id`, rebuilding only its ancestors; identity is kept when unchanged. */
function replaceNode(node: TreeNode, id: string, replace: (node: TreeNode) => TreeNode): TreeNode {
  if (node.id === id) return replace(node);
  if (node.type === "leaf") return node;
  const first = replaceNode(node.children[0], id, replace);
  const second = replaceNode(node.children[1], id, replace);
  if (first === node.children[0] && second === node.children[1]) return node;
  return { ...node, children: [first, second] };
}

function collectLeaves(node: TreeNode): LeafNode[] {
  if (node.type === "leaf") return [node];
  return [...collectLeaves(node.children[0]), ...collectLeaves(node.children[1])];
}

function containsLeaf(node: TreeNode, leafId: string): boolean {
  if (node.type === "leaf") return node.id === leafId;
  return containsLeaf(node.children[0], leafId) || containsLeaf(node.children[1], leafId);
}

function findParentSplit(node: TreeNode, childId: string): SplitNode | null {
  if (node.type === "leaf") return null;
  if (node.children.some((child) => child.id === childId)) return node;
  return findParentSplit(node.children[0], childId) ?? findParentSplit(node.children[1], childId);
}

function treeDepth(node: TreeNode): number {
  if (node.type === "leaf") return 0;
  return 1 + Math.max(treeDepth(node.children[0]), treeDepth(node.children[1]));
}

interface Bounds {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface PlacedLeaf {
  leaf: LeafNode;
  bounds: Bounds;
}

function center(bounds: Bounds): [number, number] {
  return [bounds.x + bounds.w / 2, bounds.y + bounds.h / 2];
}

function placeLeaves(node: TreeNode, bounds: Bounds): PlacedLeaf[] {
  if (node.type === "leaf") return [{ leaf: node, bounds }];
  const { ratio } = node;
  const [first, second]: [Bounds, Bounds] =
    node.direction === "vertical"
      ? [
          { ...bounds, w: bounds.w * ratio },
          { ...bounds, x: bounds.x + bounds.w * ratio, w: bounds.w * (1 - ratio) },
        ]
      : [
          { ...bounds, h: bounds.h * ratio },
          { ...bounds, y: bounds.y + bounds.h * ratio, h: bounds.h * (1 - ratio) },
        ];
  return [...placeLeaves(node.children[0], first), ...placeLeaves(node.children[1], second)];
}
