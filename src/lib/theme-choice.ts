import type { ThemeChoice, ThemeSource } from "./settings.svelte";

export type ThemeSide = "light" | "dark";

/** The source a mode uses; a plain CSS theme name applies to both modes. */
export function sideSource(choice: ThemeChoice, side: ThemeSide): ThemeSource {
  return typeof choice === "string" ? { css: choice } : choice[side];
}

/** The CSS theme used in both modes, if any. */
export function sharedCssTheme(choice: ThemeChoice): string | null {
  if (typeof choice === "string") return choice;
  const { light, dark } = choice;
  return typeof light !== "string" && typeof dark !== "string" && light.css === dark.css
    ? light.css
    : null;
}

/** Replaces one mode's source; the same CSS theme in both modes collapses to its plain name. */
export function withSideSource(
  choice: ThemeChoice,
  side: ThemeSide,
  source: ThemeSource,
): ThemeChoice {
  const next = {
    light: sideSource(choice, "light"),
    dark: sideSource(choice, "dark"),
    [side]: source,
  };
  return sharedCssTheme(next) ?? next;
}

/** A source as a single string, for use as a select value. */
export function sourceKey(source: ThemeSource): string {
  return typeof source === "string" ? `scheme:${source}` : `css:${source.css}`;
}

export function parseSourceKey(key: string): ThemeSource {
  return key.startsWith("css:") ? { css: key.slice(4) } : key.slice("scheme:".length);
}
