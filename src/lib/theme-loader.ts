import type { ITheme } from "@xterm/xterm";
import { preferences } from "./api";

const STYLE_ID = "planeai-theme";

export const DEFAULT_THEME_CSS = `:root {
  /* Calm Light — surface scale maps semantic roles */
  /* 50=main/content, 100=chrome/titlebar/footers, 200=panelHi/hover, 300=border stronger, 400=text-3, 500=text-2, 600=text-2 alt, 700=text-1 darker, 800=heading, 900=text-1, 950=deepest */
  --color-surface-50: #ffffff;
  --color-surface-100: #f6f5f3;
  --color-surface-200: #f2f1ee;
  --color-surface-300: rgba(0,0,0,0.12);
  --color-surface-400: #9298a0;
  --color-surface-500: #5e636b;
  --color-surface-600: #5e636b;
  --color-surface-700: #1b1c1e;
  --color-surface-800: #1b1c1e;
  --color-surface-900: #1b1c1e;
  --color-surface-950: #eceae6;
  /* Primary = indigo accent */
  --color-primary-50: #f0f1ff;
  --color-primary-100: #e0e3ff;
  --color-primary-200: #c4c9ff;
  --color-primary-300: #a0a8ff;
  --color-primary-400: #8e98ff;
  --color-primary-500: #7c8cff;
  --color-primary-600: #6670d9;
  --color-primary-700: #5158b3;
  --color-primary-800: #3c418c;
  --color-primary-900: #2b2f66;
  --color-primary-950: #1a1c40;
  /* Status */
  --color-error-50: #fef2f1;
  --color-error-100: #fdd8d5;
  --color-error-200: #f5b3ad;
  --color-error-300: #ec8e85;
  --color-error-400: #e07d6e;
  --color-error-500: #dd7a6b;
  --color-error-600: #c4564a;
  --color-error-700: #9e3d33;
  --color-error-800: #782e26;
  --color-error-900: #52201a;
  --color-error-950: #2c110e;
  --color-warning-50: #fef7ec;
  --color-warning-100: #fdebc8;
  --color-warning-200: #f5d48e;
  --color-warning-300: #e8ad5f;
  --color-warning-400: #d4963a;
  --color-warning-500: #b87e28;
  /* Semantic extras */
  --theme-border: #f0eee9;
  --theme-border-strong: #e8e6e1;
  --theme-text-1: #1b1c1e;
  --theme-text-2: #5e636b;
  --theme-text-3: #9298a0;
  --theme-accent-bg: rgba(124,140,255,0.12);
  --theme-status-running: #56cf8b;
  --theme-status-review: #e8ad5f;
  --theme-status-exited: #e07d6e;
  --theme-status-idle: #9298a0;
  --radius-base: 0.4375rem;
  --radius-container: 0.75rem;
  --terminal-background: #fbfaf7;
  --terminal-foreground: #3b3e44;
  --terminal-cursor: #5a63d8;
  --terminal-selection: rgba(124,140,255,0.18);
  --terminal-black: #3b3e44;
  --terminal-red: #cf4f3e;
  --terminal-green: #2f9e60;
  --terminal-yellow: #9a7a1c;
  --terminal-blue: #5a63d8;
  --terminal-magenta: #8250df;
  --terminal-cyan: #2f86a8;
  --terminal-white: #9aa0a8;
  --terminal-bright-black: #5e636b;
  --terminal-bright-red: #e07d6e;
  --terminal-bright-green: #56cf8b;
  --terminal-bright-yellow: #e8ad5f;
  --terminal-bright-blue: #7c8cff;
  --terminal-bright-magenta: #b08cff;
  --terminal-bright-cyan: #5fb6d6;
  --terminal-bright-white: #1b1c1e;
  --editor-background: #ffffff;
  --editor-foreground: #3b3e44;
  --editor-selection: rgba(124,140,255,0.18);
  --editor-line-number: #9298a0;
  --editor-added: #2f9e60;
  --editor-deleted: #cf4f3e;
  --editor-added-bg: rgba(47,158,96,0.11);
  --editor-deleted-bg: rgba(207,79,62,0.11);
  --editor-keyword: #5a63d8;
  --editor-string: #2f86a8;
  --editor-comment: #9298a0;
  --editor-number: #9a7a1c;
  --editor-variable: #3b3e44;
  --editor-type: #8250df;
  --editor-function: #8250df;
  --editor-property: #5a63d8;
  --editor-operator: #cf4f3e;
  --editor-punctuation: #3b3e44;
  --editor-meta: #2f86a8;
}
.dark {
  /* Calm Dark — surface scale maps semantic roles */
  /* 50=text-1, 100=text-1 alt, 200=text-2, 300=border stronger, 400=text-3, 500=text-2, 600=panelHi, 700=panel/card, 800=chrome/sidebar, 900=canvas, 950=main/terminal deepest */
  --color-surface-50: #e7e9ed;
  --color-surface-100: #e7e9ed;
  --color-surface-200: #9ba1aa;
  --color-surface-300: rgba(255,255,255,0.12);
  --color-surface-400: #646a73;
  --color-surface-500: #9ba1aa;
  --color-surface-600: #23262c;
  --color-surface-700: #202329;
  --color-surface-800: #1b1d22;
  --color-surface-900: #15161a;
  --color-surface-950: #101114;
  /* Primary = indigo accent */
  --color-primary-50: #1a1c40;
  --color-primary-100: #2b2f66;
  --color-primary-200: #3c418c;
  --color-primary-300: #5158b3;
  --color-primary-400: #6670d9;
  --color-primary-500: #7c8cff;
  --color-primary-600: #8e98ff;
  --color-primary-700: #a0a8ff;
  --color-primary-800: #c4c9ff;
  --color-primary-900: #e0e3ff;
  --color-primary-950: #f0f1ff;
  /* Status */
  --color-error-50: #2c110e;
  --color-error-100: #52201a;
  --color-error-200: #782e26;
  --color-error-300: #9e3d33;
  --color-error-400: #e07d6e;
  --color-error-500: #dd7a6b;
  --color-error-600: #ec8e85;
  --color-error-700: #f5b3ad;
  --color-error-800: #fdd8d5;
  --color-error-900: #fef2f1;
  --color-error-950: #ffffff;
  --color-warning-50: #2c1a06;
  --color-warning-100: #523108;
  --color-warning-200: #7a4a12;
  --color-warning-300: #b87e28;
  --color-warning-400: #e8ad5f;
  --color-warning-500: #e8ad5f;
  /* Semantic extras */
  --theme-border: #1a1b1f;
  --theme-border-strong: #1e2025;
  --theme-text-1: #e7e9ed;
  --theme-text-2: #9ba1aa;
  --theme-text-3: #646a73;
  --theme-accent-bg: rgba(124,140,255,0.16);
  --theme-status-running: #56cf8b;
  --theme-status-review: #e8ad5f;
  --theme-status-exited: #e07d6e;
  --theme-status-idle: #646a73;
  --terminal-background: #0e0f12;
  --terminal-foreground: #d6d9de;
  --terminal-cursor: #8ea0ff;
  --terminal-selection: rgba(124,140,255,0.25);
  --terminal-black: #484f58;
  --terminal-red: #ec7e6f;
  --terminal-green: #67d391;
  --terminal-yellow: #e3c07b;
  --terminal-blue: #8ea0ff;
  --terminal-magenta: #b08cff;
  --terminal-cyan: #5fb6d6;
  --terminal-white: #d6d9de;
  --terminal-bright-black: #7e8590;
  --terminal-bright-red: #f5a89e;
  --terminal-bright-green: #56cf8b;
  --terminal-bright-yellow: #e8ad5f;
  --terminal-bright-blue: #7c8cff;
  --terminal-bright-magenta: #c4a8ff;
  --terminal-bright-cyan: #7ecde6;
  --terminal-bright-white: #e7e9ed;
  --editor-background: #101114;
  --editor-foreground: #d6d9de;
  --editor-selection: rgba(124,140,255,0.25);
  --editor-line-number: #646a73;
  --editor-added: #67d391;
  --editor-deleted: #ec7e6f;
  --editor-added-bg: rgba(103,211,145,0.13);
  --editor-deleted-bg: rgba(236,126,111,0.13);
  --editor-keyword: #ec7e6f;
  --editor-string: #5fb6d6;
  --editor-comment: #7e8590;
  --editor-number: #e3c07b;
  --editor-variable: #d6d9de;
  --editor-type: #b08cff;
  --editor-function: #b08cff;
  --editor-property: #8ea0ff;
  --editor-operator: #ec7e6f;
  --editor-punctuation: #d6d9de;
  --editor-meta: #5fb6d6;
}`;

export function injectTheme(css: string): void {
  let el = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = STYLE_ID;
    document.head.appendChild(el);
  }
  el.textContent = css || DEFAULT_THEME_CSS;
}

export function extractTerminalTheme(): ITheme {
  const s = getComputedStyle(document.documentElement);
  const v = (name: string) => s.getPropertyValue(name).trim();
  return {
    background: v("--terminal-background"),
    foreground: v("--terminal-foreground"),
    cursor: v("--terminal-cursor"),
    selectionBackground: v("--terminal-selection"),
    black: v("--terminal-black"),
    red: v("--terminal-red"),
    green: v("--terminal-green"),
    yellow: v("--terminal-yellow"),
    blue: v("--terminal-blue"),
    magenta: v("--terminal-magenta"),
    cyan: v("--terminal-cyan"),
    white: v("--terminal-white"),
    brightBlack: v("--terminal-bright-black"),
    brightRed: v("--terminal-bright-red"),
    brightGreen: v("--terminal-bright-green"),
    brightYellow: v("--terminal-bright-yellow"),
    brightBlue: v("--terminal-bright-blue"),
    brightMagenta: v("--terminal-bright-magenta"),
    brightCyan: v("--terminal-bright-cyan"),
    brightWhite: v("--terminal-bright-white"),
  };
}

export async function loadTheme(): Promise<void> {
  try {
    const css = await preferences.getThemeCss();
    injectTheme(css);
  } catch {
    injectTheme("");
  }
}
