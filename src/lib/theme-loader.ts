import type { ITheme } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";

const STYLE_ID = "planeai-theme";

export const DEFAULT_THEME_CSS = `:root {
  --color-surface-50: hsl(0 0% 100%);
  --color-surface-100: hsl(240 5% 96%);
  --color-surface-200: hsl(240 5% 92%);
  --color-surface-300: hsl(240 4% 85%);
  --color-surface-400: hsl(240 4% 70%);
  --color-surface-500: hsl(240 3% 55%);
  --color-surface-600: hsl(240 3% 42%);
  --color-surface-700: hsl(240 4% 32%);
  --color-surface-800: hsl(240 5% 20%);
  --color-surface-900: hsl(240 6% 10%);
  --color-surface-950: hsl(0 0% 5%);
  --color-primary-50: hsl(240 5% 96%);
  --color-primary-100: hsl(240 5% 90%);
  --color-primary-200: hsl(240 5% 82%);
  --color-primary-300: hsl(240 5% 70%);
  --color-primary-400: hsl(240 5% 55%);
  --color-primary-500: hsl(240 6% 10%);
  --color-primary-600: hsl(240 6% 8%);
  --color-primary-700: hsl(240 6% 6%);
  --color-primary-800: hsl(0 0% 4%);
  --color-primary-900: hsl(0 0% 2%);
  --color-primary-950: hsl(0 0% 0%);
  --color-error-50: hsl(347 77% 95%);
  --color-error-100: hsl(347 77% 88%);
  --color-error-200: hsl(347 77% 78%);
  --color-error-300: hsl(347 77% 68%);
  --color-error-400: hsl(347 77% 58%);
  --color-error-500: hsl(347 77% 50%);
  --color-error-600: hsl(347 77% 42%);
  --color-error-700: hsl(347 70% 35%);
  --color-error-800: hsl(347 65% 28%);
  --color-error-900: hsl(347 60% 22%);
  --color-error-950: hsl(347 55% 15%);
  --color-warning-50: hsl(38 92% 95%);
  --color-warning-100: hsl(38 92% 85%);
  --color-warning-200: hsl(38 92% 72%);
  --color-warning-300: hsl(38 92% 60%);
  --color-warning-400: hsl(38 92% 50%);
  --color-warning-500: hsl(38 80% 42%);
  --radius-base: 0.375rem;
  --radius-container: 0.75rem;
  --terminal-background: #ffffff;
  --terminal-foreground: #171717;
  --terminal-cursor: #0969da;
  --terminal-selection: #dbe9f9;
  --terminal-black: #24292f;
  --terminal-red: #cf222e;
  --terminal-green: #116329;
  --terminal-yellow: #4d2d00;
  --terminal-blue: #0550ae;
  --terminal-magenta: #8250df;
  --terminal-cyan: #1b7c83;
  --terminal-white: #6e7781;
  --terminal-bright-black: #57606a;
  --terminal-bright-red: #a40e26;
  --terminal-bright-green: #1a7f37;
  --terminal-bright-yellow: #633c01;
  --terminal-bright-blue: #0969da;
  --terminal-bright-magenta: #6639ba;
  --terminal-bright-cyan: #3192aa;
  --terminal-bright-white: #8c959f;
  --editor-background: #ffffff;
  --editor-foreground: #171717;
  --editor-selection: #dbe9f9;
  --editor-line-number: #6e7781;
  --editor-added: #116329;
  --editor-deleted: #cf222e;
}
.dark {
  --color-surface-50: hsl(0 0% 95%);
  --color-surface-100: hsl(0 0% 92%);
  --color-surface-200: hsl(0 0% 82%);
  --color-surface-300: hsl(0 0% 70%);
  --color-surface-400: hsl(240 3% 55%);
  --color-surface-500: hsl(240 3% 40%);
  --color-surface-600: hsl(240 3% 25%);
  --color-surface-700: hsl(240 4% 16%);
  --color-surface-800: hsl(240 4% 12%);
  --color-surface-900: hsl(0 0% 8%);
  --color-surface-950: hsl(0 0% 5%);
  --color-primary-50: hsl(0 0% 4%);
  --color-primary-100: hsl(0 0% 10%);
  --color-primary-200: hsl(0 0% 20%);
  --color-primary-300: hsl(0 0% 35%);
  --color-primary-400: hsl(0 0% 50%);
  --color-primary-500: hsl(0 0% 96%);
  --color-primary-600: hsl(0 0% 92%);
  --color-primary-700: hsl(0 0% 88%);
  --color-primary-800: hsl(0 0% 95%);
  --color-primary-900: hsl(0 0% 98%);
  --color-primary-950: hsl(0 0% 100%);
  --terminal-background: #0d0d0d;
  --terminal-foreground: #f2f2f2;
  --terminal-cursor: #58a6ff;
  --terminal-selection: #264f78;
  --terminal-black: #484f58;
  --terminal-red: #ff7b72;
  --terminal-green: #3fb950;
  --terminal-yellow: #d29922;
  --terminal-blue: #58a6ff;
  --terminal-magenta: #bc8cff;
  --terminal-cyan: #39c5cf;
  --terminal-white: #b1bac4;
  --terminal-bright-black: #6e7681;
  --terminal-bright-red: #ffa198;
  --terminal-bright-green: #56d364;
  --terminal-bright-yellow: #e3b341;
  --terminal-bright-blue: #79c0ff;
  --terminal-bright-magenta: #d2a8ff;
  --terminal-bright-cyan: #56d4dd;
  --terminal-bright-white: #f0f6fc;
  --editor-background: #0d0d0d;
  --editor-foreground: #f2f2f2;
  --editor-selection: #264f78;
  --editor-line-number: #6e7681;
  --editor-added: #3fb950;
  --editor-deleted: #ff7b72;
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
    const css = await invoke<string>("get_theme_css");
    injectTheme(css);
  } catch {
    injectTheme("");
  }
}
