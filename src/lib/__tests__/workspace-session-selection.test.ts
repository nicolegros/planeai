import { describe, expect, it } from "vitest";
import appSource from "../../App.svelte?raw";

describe("workspace session selection", () => {
  it("activates an already-loaded session tab when the active session changes", () => {
    expect(appSource).toMatch(
      /if \(hasActive\) \{\s*splitTree\.focusTab\(activeSessionId\);\s*lastTreeSessionId = activeSessionId;/,
    );
  });

  it("only treats the primary agent tab as an already-loaded session terminal", () => {
    expect(appSource).toMatch(
      /const hasActive = allLeaves\.some\(\(leaf\) =>\s*leaf\.tabs\.some\(\(t\) => t\.ptyKey === activeSessionId\)\s*\);/,
    );
  });

  it("does not let a terminal from the previous layout reclaim focus during selection", () => {
    expect(appSource).toMatch(
      /focused=\{isActiveInLeaf && sessionId === activeSessionId && !activePluginId/,
    );
  });
});
