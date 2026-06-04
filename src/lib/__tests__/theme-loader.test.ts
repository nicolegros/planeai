import { describe, it, expect, beforeEach } from "vitest";
import { injectTheme, extractTerminalTheme, DEFAULT_THEME_CSS } from "../theme-loader";

describe("injectTheme", () => {
  beforeEach(() => {
    document.head.innerHTML = "";
  });

  it("injects a <style> tag into <head>", () => {
    injectTheme(":root { --color-surface-50: #fff; }");

    const style = document.getElementById("planeai-theme") as HTMLStyleElement;
    expect(style).not.toBeNull();
    expect(style.textContent).toBe(":root { --color-surface-50: #fff; }");
  });

  it("replaces existing theme style on re-inject", () => {
    injectTheme(":root { --color-surface-50: #fff; }");
    injectTheme(":root { --color-surface-50: #000; }");

    const styles = document.querySelectorAll("#planeai-theme");
    expect(styles.length).toBe(1);
    expect(styles[0].textContent).toBe(":root { --color-surface-50: #000; }");
  });
});

describe("extractTerminalTheme", () => {
  beforeEach(() => {
    document.head.innerHTML = "";
  });

  it("reads --terminal-* CSS vars and returns an ITheme object", () => {
    injectTheme(`
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
    `);

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
  });
});

describe("injectTheme fallback", () => {
  beforeEach(() => {
    document.head.innerHTML = "";
  });

  it("uses embedded default when given empty string", () => {
    injectTheme("");

    const style = document.getElementById("planeai-theme") as HTMLStyleElement;
    expect(style).not.toBeNull();
    expect(style.textContent).toBe(DEFAULT_THEME_CSS);
    expect(style.textContent!.length).toBeGreaterThan(0);
  });
});
