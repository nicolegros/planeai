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
