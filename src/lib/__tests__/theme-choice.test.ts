import { describe, expect, it } from "vitest";
import {
  parseSourceKey,
  sharedCssTheme,
  sideSource,
  sourceKey,
  withSideSource,
} from "../theme-choice";

describe("theme choice", () => {
  it("reads a plain CSS theme as the source of both modes", () => {
    expect(sideSource("default", "light")).toEqual({ css: "default" });
    expect(sideSource("default", "dark")).toEqual({ css: "default" });
  });

  it("keeps the other mode when one mode picks a scheme", () => {
    expect(withSideSource("default", "dark", "Dracula")).toEqual({
      light: { css: "default" },
      dark: "Dracula",
    });
  });

  it("collapses the same CSS theme in both modes to its name", () => {
    expect(withSideSource({ light: { css: "one" }, dark: "Dracula" }, "dark", { css: "one" })).toBe(
      "one",
    );
  });

  it("keeps two different CSS themes per mode", () => {
    expect(withSideSource("one", "dark", { css: "dracula" })).toEqual({
      light: { css: "one" },
      dark: { css: "dracula" },
    });
  });

  it("names the CSS theme shared by both modes", () => {
    expect(sharedCssTheme("one")).toBe("one");
    expect(sharedCssTheme({ light: { css: "one" }, dark: { css: "one" } })).toBe("one");
    expect(sharedCssTheme({ light: { css: "one" }, dark: "Dracula" })).toBeNull();
  });

  it("round-trips sources through select values", () => {
    expect(sourceKey({ css: "default" })).toBe("css:default");
    expect(sourceKey("Dracula")).toBe("scheme:Dracula");
    expect(parseSourceKey("css:default")).toEqual({ css: "default" });
    expect(parseSourceKey("scheme:Dracula")).toBe("Dracula");
  });
});
