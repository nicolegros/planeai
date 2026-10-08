import { describe, expect, it } from "vitest";

const sourceFiles = import.meta.glob(
  ["../../components/UnifiedSidebar.svelte", "../../App.svelte"],
  {
    eager: true,
    query: "?raw",
    import: "default",
  },
) as Record<string, string>;
const sidebar = sourceFiles["../../components/UnifiedSidebar.svelte"];
const app = sourceFiles["../../App.svelte"];

describe("plugin session indicators", () => {
  it("derives running session indicators and passes them to the sidebar", () => {
    expect(app).toMatch(
      /const sessionIndicatorContributions = \$derived\([\s\S]*?placement === "session\.indicator"/,
    );
    expect(app).toContain("{sessionIndicatorContributions}");
  });

  it("mounts indicator contributions beside every session-row shape with that row context", () => {
    expect(sidebar).toMatch(
      /data-plugin-session-indicator=\{childSession\.id\}[\s\S]*?session=\{pluginSessionContext\(childSession\)\}/,
    );
    expect(sidebar).toMatch(
      /data-plugin-session-indicator=\{session\.id\}[\s\S]*?session=\{pluginSessionContext\(session\)\}/,
    );
    expect(sidebar).toMatch(
      /data-plugin-session-indicator=\{linked\.id\}[\s\S]*?session=\{pluginSessionContext\(linked\)\}/,
    );
    expect(sidebar).toContain("relative flex items-center gap-1.5");
    expect(sidebar).toContain(
      "absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 pointer-events-none",
    );
    expect(sidebar).toContain("pr-2");
    expect(sidebar).toContain(
      ':global(.session-row:has([data-plugin-indicator-visible="true"]) > button)',
    );
  });

  it("does not add visual-only indicators to the keyboard navigation model", () => {
    expect(sidebar).not.toMatch(/type: "session_indicator"/);
    expect(sidebar).toContain("function pluginSessionContext(session: Session)");
  });
});
