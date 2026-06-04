import type { ITheme } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";

const STYLE_ID = "planeai-theme";

export const DEFAULT_THEME_CSS = `:root {
  --color-surface-50: oklch(98% 0.01 260);
  --color-surface-100: oklch(95% 0.01 260);
  --color-surface-200: oklch(90% 0.01 260);
  --color-surface-300: oklch(84% 0.01 260);
  --color-surface-400: oklch(72% 0.01 260);
  --color-surface-500: oklch(60% 0.01 260);
  --color-surface-600: oklch(50% 0.01 260);
  --color-surface-700: oklch(40% 0.02 260);
  --color-surface-800: oklch(30% 0.02 260);
  --color-surface-900: oklch(20% 0.02 260);
  --color-surface-950: oklch(13% 0.02 260);
  --color-primary-50: oklch(95% 0.05 258);
  --color-primary-100: oklch(88% 0.08 258);
  --color-primary-200: oklch(80% 0.12 258);
  --color-primary-300: oklch(72% 0.15 258);
  --color-primary-400: oklch(64% 0.18 258);
  --color-primary-500: oklch(56% 0.21 258);
  --color-primary-600: oklch(48% 0.19 258);
  --color-primary-700: oklch(40% 0.16 258);
  --color-primary-800: oklch(32% 0.13 258);
  --color-primary-900: oklch(25% 0.10 258);
  --color-primary-950: oklch(18% 0.07 258);
  --color-error-50: oklch(95% 0.03 25);
  --color-error-100: oklch(88% 0.06 25);
  --color-error-200: oklch(80% 0.10 25);
  --color-error-300: oklch(72% 0.14 25);
  --color-error-400: oklch(64% 0.18 25);
  --color-error-500: oklch(56% 0.22 25);
  --color-error-600: oklch(48% 0.20 25);
  --color-error-700: oklch(40% 0.17 25);
  --color-error-800: oklch(33% 0.14 25);
  --color-error-900: oklch(26% 0.11 25);
  --color-error-950: oklch(19% 0.08 25);
  --color-warning-50: oklch(95% 0.04 85);
  --color-warning-100: oklch(88% 0.07 85);
  --color-warning-200: oklch(80% 0.11 80);
  --color-warning-300: oklch(72% 0.14 75);
  --color-warning-400: oklch(64% 0.14 70);
  --color-warning-500: oklch(56% 0.13 65);
  --radius-base: 0.375rem;
  --radius-container: 0.75rem;
  --terminal-background: #1e1e2e;
  --terminal-foreground: #cdd6f4;
  --terminal-cursor: #f5e0dc;
  --terminal-selection: #45475a;
  --terminal-black: #45475a;
  --terminal-red: #f38ba8;
  --terminal-green: #a6e3a1;
  --terminal-yellow: #f9e2af;
  --terminal-blue: #89b4fa;
  --terminal-magenta: #f5c2e7;
  --terminal-cyan: #94e2d5;
  --terminal-white: #bac2de;
  --terminal-bright-black: #585b70;
  --terminal-bright-red: #f38ba8;
  --terminal-bright-green: #a6e3a1;
  --terminal-bright-yellow: #f9e2af;
  --terminal-bright-blue: #89b4fa;
  --terminal-bright-magenta: #f5c2e7;
  --terminal-bright-cyan: #94e2d5;
  --terminal-bright-white: #a6adc8;
  --editor-background: #1e1e2e;
  --editor-foreground: #cdd6f4;
  --editor-selection: #45475a;
  --editor-line-number: #6c7086;
  --editor-added: #a6e3a1;
  --editor-deleted: #f38ba8;
}
.dark {
  --color-surface-50: oklch(22% 0.02 260);
  --color-surface-100: oklch(24% 0.02 260);
  --color-surface-200: oklch(28% 0.02 260);
  --color-surface-300: oklch(33% 0.02 260);
  --color-surface-400: oklch(40% 0.02 260);
  --color-surface-500: oklch(50% 0.01 260);
  --color-surface-600: oklch(60% 0.01 260);
  --color-surface-700: oklch(72% 0.01 260);
  --color-surface-800: oklch(84% 0.01 260);
  --color-surface-900: oklch(92% 0.01 260);
  --color-surface-950: oklch(96% 0.01 260);
  --terminal-background: #1e1e2e;
  --terminal-foreground: #cdd6f4;
  --terminal-cursor: #f5e0dc;
  --terminal-selection: #45475a;
  --terminal-black: #45475a;
  --terminal-red: #f38ba8;
  --terminal-green: #a6e3a1;
  --terminal-yellow: #f9e2af;
  --terminal-blue: #89b4fa;
  --terminal-magenta: #f5c2e7;
  --terminal-cyan: #94e2d5;
  --terminal-white: #bac2de;
  --terminal-bright-black: #585b70;
  --terminal-bright-red: #f38ba8;
  --terminal-bright-green: #a6e3a1;
  --terminal-bright-yellow: #f9e2af;
  --terminal-bright-blue: #89b4fa;
  --terminal-bright-magenta: #f5c2e7;
  --terminal-bright-cyan: #94e2d5;
  --terminal-bright-white: #a6adc8;
  --editor-background: #1e1e2e;
  --editor-foreground: #cdd6f4;
  --editor-selection: #45475a;
  --editor-line-number: #6c7086;
  --editor-added: #a6e3a1;
  --editor-deleted: #f38ba8;
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
