export type PluginSidebarNavRow = {
  id: string;
  onSelect?: () => void;
  onCollapse?: () => void;
  onExpand?: () => void;
  onFocus?: (selected: boolean) => void;
};

export type PluginSidebarContribution = {
  key: string;
  rows: PluginSidebarNavRow[];
};

let contributions = $state<Record<string, PluginSidebarContribution>>({});

export function registerPluginSidebarContribution(
  key: string,
  rows: PluginSidebarNavRow[],
): () => void {
  contributions = { ...contributions, [key]: { key, rows } };
  return () => {
    const next = { ...contributions };
    delete next[key];
    contributions = next;
  };
}

export function getPluginSidebarRows(key: string): PluginSidebarNavRow[] {
  return contributions[key]?.rows ?? [];
}

export function getAllPluginSidebarContributions(): PluginSidebarContribution[] {
  return Object.values(contributions);
}
