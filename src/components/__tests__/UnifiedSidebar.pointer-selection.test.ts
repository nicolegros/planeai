import { describe, expect, it } from "vitest";
import sidebarSource from "../UnifiedSidebar.svelte?raw";

describe("UnifiedSidebar pointer session selection", () => {
  it("does not auto-focus and scroll the active row on pointerdown before a row click", () => {
    expect(sidebarSource).toMatch(
      /function handleSidebarPointerDown\(\): void \{\s*skipNextAutoFocus = true;\s*focusSidebar\(\);\s*\}/,
    );
    expect(sidebarSource).toMatch(
      /if \(skipNextAutoFocus \|\| zone !== "sidebar" \|\| !navRef\) return;/,
    );
    expect(sidebarSource).toMatch(
      /if \(skipNextAutoFocus\) \{\s*skipNextAutoFocus = false;\s*return;\s*\}/,
    );
    expect(sidebarSource).toContain("onpointerdown={handleSidebarPointerDown}");
  });
});
