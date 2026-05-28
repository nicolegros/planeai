export interface Tab {
  index: number;
  label: string;
}

const sessions = new Map<string, Tab[]>();
const activeTabs = new Map<string, number>();
const nextIndex = new Map<string, number>();

export function initSession(sessionId: string, tabCount = 1): void {
  const tabs: Tab[] = [{ index: 0, label: "Agent" }];
  for (let i = 1; i < tabCount; i++) {
    tabs.push({ index: i, label: `Shell ${i}` });
  }
  sessions.set(sessionId, tabs);
  activeTabs.set(sessionId, 0);
  nextIndex.set(sessionId, tabCount);
}

export function getTabs(sessionId: string): Tab[] {
  return sessions.get(sessionId) ?? [];
}

export function addTab(sessionId: string): number {
  const tabs = sessions.get(sessionId);
  if (!tabs) return -1;
  const index = nextIndex.get(sessionId) ?? tabs.length;
  tabs.push({ index, label: `Shell ${index}` });
  nextIndex.set(sessionId, index + 1);
  return index;
}

export function removeTab(sessionId: string, tabIndex: number): void {
  if (tabIndex === 0) return;
  const tabs = sessions.get(sessionId);
  if (!tabs) return;
  const pos = tabs.findIndex((t) => t.index === tabIndex);
  if (pos === -1) return;
  tabs.splice(pos, 1);
  if (activeTabs.get(sessionId) === tabIndex) {
    const prev = tabs[pos - 1] ?? tabs[0];
    activeTabs.set(sessionId, prev?.index ?? 0);
  }
}

export function setActiveTab(sessionId: string, tabIndex: number): void {
  activeTabs.set(sessionId, tabIndex);
}

export function getActiveTabIndex(sessionId: string): number {
  return activeTabs.get(sessionId) ?? 0;
}

export function getTabCount(sessionId: string): number {
  return sessions.get(sessionId)?.length ?? 0;
}

export function destroySession(sessionId: string): void {
  sessions.delete(sessionId);
  activeTabs.delete(sessionId);
  nextIndex.delete(sessionId);
}
