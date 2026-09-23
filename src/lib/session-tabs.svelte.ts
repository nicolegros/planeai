export interface Tab {
  /** Stable UI identity when numeric operation indices are not unique. */
  id?: string;
  index: number;
  label: string;
  icon?: string;
  modified?: boolean;
  customTitle?: boolean;
}

interface SessionTabState {
  tabs: Tab[];
  activeTab: number;
  nextIndex: number;
}

let state = $state<Record<string, SessionTabState>>({});

function relabel(tabs: Tab[]): Tab[] {
  let shellNum = 1;
  return tabs.map((t) => {
    if (t.index === 0) return t;
    const num = shellNum++;
    if (t.customTitle) return t;
    return { ...t, label: `Shell ${num}`, icon: "terminal" };
  });
}

export function initSession(sessionId: string, tabCount = 1): void {
  const tabs: Tab[] = [{ index: 0, label: "Agent", icon: "bot" }];
  for (let i = 1; i < tabCount; i++) {
    tabs.push({ index: i, label: `Shell ${i}`, icon: "terminal" });
  }
  state[sessionId] = { tabs, activeTab: 0, nextIndex: tabCount };
}

export function getTabs(sessionId: string): Tab[] {
  return state[sessionId]?.tabs ?? [];
}

export function addTab(sessionId: string): number {
  const s = state[sessionId];
  if (!s) return -1;
  // Never hand out an index already in use. `nextIndex` is seeded from the
  // persisted tab count, which can be lower than the highest index in a restored
  // layout — reusing one produces a duplicate pty key, which both breaks the
  // keyed tab list and makes a new tab silently resolve to the existing one.
  const highest = s.tabs.reduce((max, tab) => Math.max(max, tab.index), 0);
  const index = Math.max(s.nextIndex, highest + 1);
  s.tabs = relabel([...s.tabs, { index, label: "", icon: "terminal" }]);
  s.nextIndex = index + 1;
  return index;
}

/**
 * Adopt shell tab indices restored from a persisted layout.
 *
 * The layout is the authority on which tabs exist; this state is rebuilt from the
 * tab count at startup and would otherwise hand out an index the layout already
 * uses. Adopting keeps the two views consistent and the allocator safe.
 */
export function adoptTabIndices(sessionId: string, indices: number[]): void {
  const s = state[sessionId];
  if (!s) return;
  const known = new Set(s.tabs.map((tab) => tab.index));
  const adopted = indices.filter((index) => index > 0 && !known.has(index));
  if (adopted.length > 0) {
    const tabs = [...s.tabs];
    for (const index of adopted) tabs.push({ index, label: "", icon: "terminal" });
    tabs.sort((left, right) => left.index - right.index);
    s.tabs = relabel(tabs);
  }
  const highest = indices.reduce((max, index) => Math.max(max, index), 0);
  if (highest + 1 > s.nextIndex) s.nextIndex = highest + 1;
}

export function removeTab(sessionId: string, tabIndex: number): void {
  if (tabIndex === 0) return;
  const s = state[sessionId];
  if (!s) return;
  const pos = s.tabs.findIndex((t) => t.index === tabIndex);
  if (pos === -1) return;
  s.tabs = relabel(s.tabs.filter((t) => t.index !== tabIndex));
  if (s.activeTab === tabIndex) {
    const prev = s.tabs[pos - 1] ?? s.tabs[0];
    s.activeTab = prev?.index ?? 0;
  }
}

export function setActiveTab(sessionId: string, tabIndex: number): void {
  const s = state[sessionId];
  if (s) s.activeTab = tabIndex;
}

export function getActiveTabIndex(sessionId: string): number {
  return state[sessionId]?.activeTab ?? 0;
}

export function getTabCount(sessionId: string): number {
  return state[sessionId]?.tabs.length ?? 0;
}

export function destroySession(sessionId: string): void {
  delete state[sessionId];
}

export function setTabTitle(sessionId: string, tabIndex: number, title: string): void {
  if (tabIndex === 0) return; // Agent tab is not modifiable
  const s = state[sessionId];
  if (!s) return;
  const tab = s.tabs.find((t) => t.index === tabIndex);
  if (!tab) return;
  tab.label = title;
  tab.customTitle = true;
}
