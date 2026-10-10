import type { ITheme } from "@xterm/xterm";
import { preferences, type ThemeCss } from "./api";

const STYLE_ID = "planeai-theme";

export const NO_THEME: ThemeCss = { light: "", dark: "" };

let current = NO_THEME;

/**
 * Set the theme CSS for each appearance mode; each overrides the CSS custom properties defined in app.css.
 * Empty CSS for a mode leaves the app.css defaults in place.
 */
export function injectTheme(css: ThemeCss): void {
  current = css;
  applyThemeMode();
  window.dispatchEvent(new Event("planeai-theme-changed"));
}

/** Show the CSS for the active mode; call after toggling the `dark` class on <html>. */
export function applyThemeMode(): void {
  let el = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = STYLE_ID;
    document.head.appendChild(el);
  }
  el.textContent = document.documentElement.classList.contains("dark")
    ? current.dark
    : current.light;
}

export function extractTerminalTheme(): ITheme {
  const s = getComputedStyle(document.documentElement);
  const v = (name: string) => s.getPropertyValue(name).trim();
  // Hand-written CSS themes omit these; undefined keeps xterm's defaults.
  const optional = (name: string) => v(name) || undefined;
  return {
    background: v("--terminal-background"),
    foreground: v("--terminal-foreground"),
    cursor: v("--terminal-cursor"),
    cursorAccent: optional("--terminal-cursor-text"),
    selectionBackground: v("--terminal-selection"),
    selectionForeground: optional("--terminal-selection-foreground"),
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
    injectTheme(NO_THEME);
  }
}
