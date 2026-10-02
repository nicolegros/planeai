const PREFIX = "planeai:layout:";

export function getLayoutWidth(key: string, defaultWidth: number): number {
  const raw = localStorage.getItem(PREFIX + key);
  if (raw === null) return defaultWidth;
  const val = Number(raw);
  return Number.isFinite(val) ? val : defaultWidth;
}

export function setLayoutWidth(key: string, width: number): void {
  localStorage.setItem(PREFIX + key, String(width));
}

const COLLAPSED_KEY = PREFIX + "sidebar-collapsed";

/** Sidebar sections the user explicitly collapsed or expanded, keyed by section key. */
export function getCollapsedSections(): Record<string, boolean> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).filter(([, v]) => typeof v === "boolean"));
  } catch {
    return {};
  }
}

export function setCollapsedSections(sections: Record<string, boolean>): void {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(sections));
  } catch {
    // Storage unavailable: collapse state stays in memory for this session.
  }
}
