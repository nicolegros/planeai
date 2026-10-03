export const DEFAULT_SCROLLBACK_LINES = 20_000;
const MIN_SCROLLBACK_LINES = 1_000;
const MAX_SCROLLBACK_LINES = 1_000_000;

/** Parses a typed scrollback size into the accepted range; `null` (the default) when not a number. */
export function parseScrollbackLines(raw: string): number | null {
  const lines = Number.parseInt(raw, 10);
  if (!Number.isFinite(lines)) return null;
  return Math.min(MAX_SCROLLBACK_LINES, Math.max(MIN_SCROLLBACK_LINES, lines));
}
