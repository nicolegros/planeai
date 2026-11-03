/** Host color of each tone; the status tones also back the plugin's `--planeai-<tone>` token, so both always agree. */
export const PLUGIN_TONE_COLORS = {
  success: "--color-status-running",
  warning: "--color-status-review",
  danger: "--color-status-exited",
  info: "--color-status-working",
  muted: "--color-t3",
} as const;
export type PluginTooltipTone = keyof typeof PLUGIN_TONE_COLORS;

export interface PluginTooltipLine {
  text: string;
  tone?: PluginTooltipTone;
}

export const PLUGIN_TOOLTIP_MAX_LINES = 12;
export const PLUGIN_TOOLTIP_MAX_TEXT = 200;

const isTone = (value: unknown): value is PluginTooltipTone =>
  typeof value === "string" && Object.hasOwn(PLUGIN_TONE_COLORS, value);

/** The lines of a frame's `{ type: "tooltip", lines }` message, or null when it is malformed. */
export function parseTooltipLines(value: unknown): PluginTooltipLine[] | null {
  if (!Array.isArray(value)) return null;
  const lines: PluginTooltipLine[] = [];
  for (const line of value.slice(0, PLUGIN_TOOLTIP_MAX_LINES)) {
    if (!line || typeof line !== "object" || typeof line.text !== "string") return null;
    const text = line.text.trim().slice(0, PLUGIN_TOOLTIP_MAX_TEXT);
    if (!text) continue;
    lines.push(isTone(line.tone) ? { text, tone: line.tone } : { text });
  }
  return lines;
}
