import type { ITheme } from "@xterm/xterm";
import { preferences } from "./api";

const STYLE_ID = "planeai-theme";

/**
 * Inject a theme CSS string into the document.
 * The theme file overrides CSS custom properties defined in app.css.
 * Passing empty string removes any custom theme (app.css defaults apply).
 */
export function injectTheme(css: string): void {
  let el = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = STYLE_ID;
    document.head.appendChild(el);
  }
  el.textContent = css;
  window.dispatchEvent(new Event("planeai-theme-changed"));
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
