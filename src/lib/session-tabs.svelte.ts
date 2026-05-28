export interface Tab {
  index: number;
  label: string;
}

interface SessionTabState {
  tabs: Tab[];
  activeTab: number;
  nextIndex: number;
}

let state = $state<Record<string, SessionTabState>>({});

export function initSession(sessionId: string, tabCount = 1): void {
  const tabs: Tab[] = [{ index: 0, label: "Agent" }];
  for (let i = 1; i < tabCount; i++) {
    tabs.push({ index: i, label: `Shell ${i}` });
  }
  state[sessionId] = { tabs, activeTab: 0, nextIndex: tabCount };
}

export function getTabs(sessionId: string): Tab[] {
  return state[sessionId]?.tabs ?? [];
}

export function addTab(sessionId: string): number {
  const s = state[sessionId];
  if (!s) return -1;
  const index = s.nextIndex;
  s.tabs = [...s.tabs, { index, label: `Shell ${index}` }];
  s.nextIndex = index + 1;
  return index;
}

export function removeTab(sessionId: string, tabIndex: number): void {
  if (tabIndex === 0) return;
  const s = state[sessionId];
  if (!s) return;
  const pos = s.tabs.findIndex((t) => t.index === tabIndex);
  if (pos === -1) return;
  s.tabs = s.tabs.filter((t) => t.index !== tabIndex);
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
