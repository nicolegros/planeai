import { describe, it, expect, beforeEach } from "vitest";
import { NO_THEME, applyThemeMode, injectTheme, extractTerminalTheme } from "../theme-loader";

const LIGHT = ":root { --color-panel: #fff; }";
const DARK = ":root { --color-panel: #000; }";

function themeStyle(): HTMLStyleElement {
  return document.getElementById("planeai-theme") as HTMLStyleElement;
}

describe("injectTheme", () => {
  beforeEach(() => {
    document.head.innerHTML = "";
    document.documentElement.classList.remove("dark");
  });

  it("injects the light CSS into a <style> tag in light mode", () => {
    injectTheme({ light: LIGHT, dark: DARK });

    expect(themeStyle().textContent).toBe(LIGHT);
  });

  it("injects the dark CSS in dark mode", () => {
    document.documentElement.classList.add("dark");
    injectTheme({ light: LIGHT, dark: DARK });

    expect(themeStyle().textContent).toBe(DARK);
  });

  it("swaps to the other mode's CSS when the mode changes", () => {
    injectTheme({ light: LIGHT, dark: DARK });
    document.documentElement.classList.add("dark");
    applyThemeMode();

    expect(document.querySelectorAll("#planeai-theme").length).toBe(1);
    expect(themeStyle().textContent).toBe(DARK);
    expect(
      getComputedStyle(document.documentElement).getPropertyValue("--color-panel").trim(),
    ).toBe("#000");
  });

  it("clears the theme when given no CSS", () => {
    injectTheme({ light: LIGHT, dark: DARK });
    injectTheme(NO_THEME);

    expect(themeStyle().textContent).toBe("");
  });
});

describe("extractTerminalTheme", () => {
  beforeEach(() => {
    document.head.innerHTML = "";
  });

  it("reads --terminal-* CSS vars and returns an ITheme object", () => {
    const css = `
      :root {
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
      }
    `;
    injectTheme({ light: css, dark: css });

    const theme = extractTerminalTheme();
    expect(theme.background).toBe("#1e1e2e");
    expect(theme.foreground).toBe("#cdd6f4");
    expect(theme.cursor).toBe("#f5e0dc");
    expect(theme.selectionBackground).toBe("#45475a");
    expect(theme.black).toBe("#45475a");
    expect(theme.red).toBe("#f38ba8");
    expect(theme.green).toBe("#a6e3a1");
    expect(theme.yellow).toBe("#f9e2af");
    expect(theme.blue).toBe("#89b4fa");
    expect(theme.magenta).toBe("#f5c2e7");
    expect(theme.cyan).toBe("#94e2d5");
    expect(theme.white).toBe("#bac2de");
    expect(theme.brightBlack).toBe("#585b70");
    expect(theme.brightWhite).toBe("#a6adc8");
    expect(theme.selectionForeground).toBeUndefined();
    expect(theme.cursorAccent).toBeUndefined();
  });

  it("reads selection foreground and cursor text when the theme defines them", () => {
    const css =
      ":root { --terminal-selection-foreground: #fbf1c7; --terminal-cursor-text: #282828; }";
    injectTheme({ light: css, dark: css });

    const theme = extractTerminalTheme();
    expect(theme.selectionForeground).toBe("#fbf1c7");
    expect(theme.cursorAccent).toBe("#282828");
  });
});
