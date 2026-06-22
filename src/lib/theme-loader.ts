import type { ITheme } from "@xterm/xterm";
import { preferences } from "./api";

const STYLE_ID = "planeai-theme";

export const DEFAULT_THEME_CSS = `:root {
  /* ── Surface scale (240-hue greys) ── */
  --color-surface-50: #ffffff;
  --color-surface-100: #f4f4f6;
  --color-surface-200: #e9e9ed;
  --color-surface-300: #d6d6db;
  --color-surface-400: #aeaeb5;
  --color-surface-500: #88888f;
  --color-surface-600: #686870;
  --color-surface-700: #4f4f57;
  --color-surface-800: #303039;
  --color-surface-900: #18181d;
  --color-surface-950: #0d0d0d;
  /* ── Role tokens (light) ── */
  --theme-canvas: #f4f4f6;
  --theme-chrome: #f4f4f6;
  --theme-sidebar: #f4f4f6;
  --theme-main: #ffffff;
  --theme-panel: #ffffff;
  --theme-panel-hi: #e9e9ed;
  --theme-text-1: #18181d;
  --theme-text-2: #686870;
  --theme-text-3: #88888f;
  --theme-border: rgba(0,0,0,0.06);
  --theme-border-strong: rgba(0,0,0,0.10);
  --theme-accent: #18181d;
  --theme-on-accent: #ffffff;
  --theme-accent-bg: rgba(24,24,29,0.07);
  --theme-scrim: rgba(25,25,30,0.30);
  --theme-idle: #aeaeb5;
  /* Tailwind --color-* overrides */
  --color-canvas: #f4f4f6;
  --color-chrome: #f4f4f6;
  --color-sidebar: #f4f4f6;
  --color-main: #ffffff;
  --color-panel: #ffffff;
  --color-panel-hi: #e9e9ed;
  --color-t1: #18181d;
  --color-t2: #686870;
  --color-t3: #88888f;
  --color-border: rgba(0,0,0,0.06);
  --color-border-s: rgba(0,0,0,0.10);
  --color-accent: #18181d;
  --color-on-accent: #ffffff;
  --color-accent-bg: rgba(24,24,29,0.07);
  --color-scrim: rgba(25,25,30,0.30);
  --color-status-running: #1a7f37;
  --color-status-review: #9a6700;
  --color-status-exited: #cf222e;
  --color-status-idle: #aeaeb5;
  /* ── Status ── */
  --theme-status-running: #1a7f37;
  --theme-status-review: #9a6700;
  --theme-status-exited: #cf222e;
  --theme-status-idle: #aeaeb5;
  /* ── Radii ── */
  --radius-base: 0.375rem;
  --radius-container: 0.75rem;
  /* ── Terminal (GitHub light) ── */
  --terminal-background: #ffffff;
  --terminal-foreground: #171717;
  --terminal-cursor: #18181d;
  --terminal-selection: rgba(24,24,29,0.12);
  --terminal-black: #171717;
  --terminal-red: #cf222e;
  --terminal-green: #116329;
  --terminal-yellow: #9a6700;
  --terminal-blue: #0550ae;
  --terminal-magenta: #8250df;
  --terminal-cyan: #1b7c83;
  --terminal-white: #6e7781;
  --terminal-bright-black: #6e7781;
  --terminal-bright-red: #cf222e;
  --terminal-bright-green: #1a7f37;
  --terminal-bright-yellow: #9a6700;
  --terminal-bright-blue: #0550ae;
  --terminal-bright-magenta: #8250df;
  --terminal-bright-cyan: #1b7c83;
  --terminal-bright-white: #171717;
  /* ── Diff ── */
  --diff-add-bg: #d4f8db;
  --diff-del-bg: #fdd8d8;
  --diff-add-color: #1a7f37;
  --diff-del-color: #cf222e;
  /* ── Editor ── */
  --editor-background: #ffffff;
  --editor-foreground: #171717;
  --editor-selection: rgba(24,24,29,0.12);
  --editor-line-number: #88888f;
  --editor-added: #116329;
  --editor-deleted: #cf222e;
  --editor-added-bg: #d4f8db;
  --editor-deleted-bg: #fdd8d8;
  --editor-keyword: #0550ae;
  --editor-string: #1b7c83;
  --editor-comment: #6e7781;
  --editor-number: #9a6700;
  --editor-variable: #171717;
  --editor-type: #8250df;
  --editor-function: #8250df;
  --editor-property: #0550ae;
  --editor-operator: #cf222e;
  --editor-punctuation: #171717;
  --editor-meta: #1b7c83;
}
.dark {
  /* ── Surface scale (240-hue greys, inverted) ── */
  --color-surface-50: #f2f2f2;
  --color-surface-100: #e0e0e0;
  --color-surface-200: #bfbfbf;
  --color-surface-300: #9e9e9e;
  --color-surface-400: #797980;
  --color-surface-500: #5b5b61;
  --color-surface-600: #404045;
  --color-surface-700: #2c2c31;
  --color-surface-800: #1f1f23;
  --color-surface-900: #171717;
  --color-surface-950: #0a0a0a;
  /* ── Role tokens (dark) ── */
  --theme-canvas: #171717;
  --theme-chrome: #171717;
  --theme-sidebar: #171717;
  --theme-main: #0a0a0a;
  --theme-panel: #1f1f23;
  --theme-panel-hi: #2c2c31;
  --theme-text-1: #f2f2f2;
  --theme-text-2: #9e9e9e;
  --theme-text-3: #5b5b61;
  --theme-border: rgba(255,255,255,0.08);
  --theme-border-strong: rgba(255,255,255,0.14);
  --theme-accent: #f5f5f5;
  --theme-on-accent: #0a0a0a;
  --theme-accent-bg: rgba(245,245,245,0.12);
  --theme-scrim: rgba(0,0,0,0.55);
  --theme-idle: #5b5b61;
  /* Tailwind --color-* overrides */
  --color-canvas: #171717;
  --color-chrome: #171717;
  --color-sidebar: #171717;
  --color-main: #0a0a0a;
  --color-panel: #1f1f23;
  --color-panel-hi: #2c2c31;
  --color-t1: #f2f2f2;
  --color-t2: #9e9e9e;
  --color-t3: #5b5b61;
  --color-border: rgba(255,255,255,0.08);
  --color-border-s: rgba(255,255,255,0.14);
  --color-accent: #f5f5f5;
  --color-on-accent: #0a0a0a;
  --color-accent-bg: rgba(245,245,245,0.12);
  --color-scrim: rgba(0,0,0,0.55);
  --color-status-running: #3fb950;
  --color-status-review: #d29922;
  --color-status-exited: #ff7b72;
  --color-status-idle: #5b5b61;
  /* ── Status ── */
  --theme-status-running: #3fb950;
  --theme-status-review: #d29922;
  --theme-status-exited: #ff7b72;
  --theme-status-idle: #5b5b61;
  /* ── Terminal (GitHub dark) ── */
  --terminal-background: #0d0d0d;
  --terminal-foreground: #f2f2f2;
  --terminal-cursor: #f5f5f5;
  --terminal-selection: rgba(245,245,245,0.18);
  --terminal-black: #484f58;
  --terminal-red: #ff7b72;
  --terminal-green: #3fb950;
  --terminal-yellow: #d29922;
  --terminal-blue: #58a6ff;
  --terminal-magenta: #bc8cff;
  --terminal-cyan: #39c5cf;
  --terminal-white: #f2f2f2;
  --terminal-bright-black: #6e7681;
  --terminal-bright-red: #ff7b72;
  --terminal-bright-green: #3fb950;
  --terminal-bright-yellow: #d29922;
  --terminal-bright-blue: #58a6ff;
  --terminal-bright-magenta: #bc8cff;
  --terminal-bright-cyan: #39c5cf;
  --terminal-bright-white: #f2f2f2;
  /* ── Diff ── */
  --diff-add-bg: #1a3d2a;
  --diff-del-bg: #3d1a1a;
  --diff-add-color: #3fb950;
  --diff-del-color: #ff7b72;
  /* ── Editor ── */
  --editor-background: #0a0a0a;
  --editor-foreground: #f2f2f2;
  --editor-selection: rgba(245,245,245,0.18);
  --editor-line-number: #5b5b61;
  --editor-added: #3fb950;
  --editor-deleted: #ff7b72;
  --editor-added-bg: #1a3d2a;
  --editor-deleted-bg: #3d1a1a;
  --editor-keyword: #ff7b72;
  --editor-string: #39c5cf;
  --editor-comment: #6e7681;
  --editor-number: #d29922;
  --editor-variable: #f2f2f2;
  --editor-type: #bc8cff;
  --editor-function: #bc8cff;
  --editor-property: #58a6ff;
  --editor-operator: #ff7b72;
  --editor-punctuation: #f2f2f2;
  --editor-meta: #39c5cf;
}`;

export function injectTheme(css: string): void {
  let el = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = STYLE_ID;
    document.head.appendChild(el);
  }
  el.textContent = css ? `${DEFAULT_THEME_CSS}\n${css}` : DEFAULT_THEME_CSS;
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
