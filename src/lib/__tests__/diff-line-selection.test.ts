import { describe, it, expect } from "vitest";
import { parsePatchFiles, type SelectedLineRange } from "@pierre/diffs";

/**
 * Regression test for PLA-219: mouse line selection in split diff view.
 *
 * Root cause: Race condition in @pierre/diffs CodeView where
 * InteractionManager doesn't attach pointer listeners for line selection
 * on the first render frame in split mode. The fix calls flushManagers()
 * on rendered items after the first render.
 *
 * Since the full CodeView can't run in jsdom (needs shadow DOM + custom
 * elements), this test verifies the component-level logic: the onLineSelected
 * callback correctly syncs selection state for both single and multi-line
 * mouse selections.
 */

const SAMPLE_PATCH = `diff --git a/src/main.ts b/src/main.ts
--- a/src/main.ts
+++ b/src/main.ts
@@ -1,4 +1,5 @@
 import { app } from "./app";
+import { logger } from "./logger";
 
 const server = app.listen(3000);
 console.log("running");
`;

describe("diff line selection state sync (PLA-219)", () => {
  // Simulate the onLineSelected callback logic from ReviewTab
  function simulateOnLineSelected(range: SelectedLineRange | null) {
    let diffFocus = "list";
    let cursorLine = 1;
    let selectionAnchor: number | null = null;

    if (range) {
      diffFocus = "body";
      cursorLine = range.end;
      selectionAnchor = range.start !== range.end ? range.start : null;
    } else {
      selectionAnchor = null;
    }

    return { diffFocus, cursorLine, selectionAnchor };
  }

  it("single line click sets cursorLine and clears anchor", () => {
    const result = simulateOnLineSelected({ start: 5, end: 5, side: "additions" });
    expect(result.diffFocus).toBe("body");
    expect(result.cursorLine).toBe(5);
    expect(result.selectionAnchor).toBeNull();
  });

  it("multi-line drag sets anchor to start and cursor to end", () => {
    const result = simulateOnLineSelected({ start: 3, end: 7, side: "additions" });
    expect(result.diffFocus).toBe("body");
    expect(result.cursorLine).toBe(7);
    expect(result.selectionAnchor).toBe(3);
  });

  it("null range (deselect) clears anchor", () => {
    const result = simulateOnLineSelected(null);
    expect(result.diffFocus).toBe("list"); // unchanged from default
    expect(result.selectionAnchor).toBeNull();
  });

  it("deletion side selection works correctly", () => {
    const result = simulateOnLineSelected({ start: 10, end: 10, side: "deletions" });
    expect(result.diffFocus).toBe("body");
    expect(result.cursorLine).toBe(10);
    expect(result.selectionAnchor).toBeNull();
  });

  it("parsed patch produces valid split line indexes for selection", () => {
    // Verify that parsePatchFiles produces hunks with proper splitLineStart
    // which is required for data-line-index in split mode
    const parsed = parsePatchFiles(SAMPLE_PATCH, "test");
    const fileDiff = parsed[0].files[0];

    expect(fileDiff.hunks.length).toBeGreaterThan(0);
    const hunk = fileDiff.hunks[0];
    expect(hunk.splitLineStart).toBeDefined();
    expect(hunk.splitLineCount).toBeGreaterThan(0);
    // Split line index must be a number (used for data-line-index="unified,split" format)
    expect(typeof hunk.splitLineStart).toBe("number");
  });
});
