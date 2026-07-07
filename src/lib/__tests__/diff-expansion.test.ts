import { describe, it, expect } from "vitest";
import { parsePatchFiles, type CodeViewItem, type DiffLineAnnotation } from "@pierre/diffs";
import { rebuildItemWithFullContent, isExpandable } from "../diff-expansion";

// A minimal unified diff patch for testing
const SAMPLE_PATCH = `diff --git a/hello.ts b/hello.ts
--- a/hello.ts
+++ b/hello.ts
@@ -1,5 +1,6 @@
 const a = 1;
 const b = 2;
+const c = 3;
 const d = 4;
 const e = 5;
 const f = 6;
`;

const OLD_CONTENT = `const a = 1;
const b = 2;
const d = 4;
const e = 5;
const f = 6;
const g = 7;
const h = 8;
const i = 9;
const j = 10;
const k = 11;
`;

const NEW_CONTENT = `const a = 1;
const b = 2;
const c = 3;
const d = 4;
const e = 5;
const f = 6;
const g = 7;
const h = 8;
const i = 9;
const j = 10;
const k = 11;
`;

function makePartialItem(): CodeViewItem<string> {
  const parsed = parsePatchFiles(SAMPLE_PATCH, "test");
  const fileDiff = parsed[0].files[0];
  return {
    id: "diff:hello.ts",
    type: "diff",
    fileDiff,
    annotations: [],
    version: 1,
  };
}

describe("diff-expansion", () => {
  describe("rebuildItemWithFullContent", () => {
    it("produces a non-partial item from full file contents", () => {
      const partial = makePartialItem();
      expect(partial.type === "diff" && partial.fileDiff.isPartial).toBe(true);

      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      expect(rebuilt).not.toBeNull();
      expect(rebuilt!.type).toBe("diff");
      if (rebuilt!.type === "diff") {
        expect(rebuilt!.fileDiff.isPartial).toBe(false);
      }
    });

    it("preserves annotations from the original item", () => {
      const partial = makePartialItem();
      const annotations: DiffLineAnnotation<string>[] = [
        { side: "additions", lineNumber: 3, metadata: "review note" },
      ];
      (partial as any).annotations = annotations;

      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      expect(rebuilt).not.toBeNull();
      expect(rebuilt!.type === "diff" && rebuilt!.annotations).toEqual(annotations);
    });

    it("increments the version number", () => {
      const partial = makePartialItem();
      partial.version = 5;

      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      expect(rebuilt!.version).toBe(6);
    });

    it("preserves the item id", () => {
      const partial = makePartialItem();
      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      expect(rebuilt!.id).toBe("diff:hello.ts");
    });

    it("preserves collapsed state", () => {
      const partial = makePartialItem();
      partial.collapsed = true;

      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      expect(rebuilt!.collapsed).toBe(true);
    });

    it("returns null for non-diff items", () => {
      const fileItem: CodeViewItem<string> = {
        id: "file:x.ts",
        type: "file",
        file: { name: "x.ts", contents: "hello" },
      };

      const result = rebuildItemWithFullContent(
        fileItem,
        { name: "x.ts", contents: "old" },
        { name: "x.ts", contents: "new" },
      );

      expect(result).toBeNull();
    });

    it("full diff contains all lines of the file for expansion", () => {
      const partial = makePartialItem();
      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      if (rebuilt!.type === "diff") {
        // Full content means additionLines contains all lines of the new file
        expect(rebuilt!.fileDiff.additionLines.length).toBe(NEW_CONTENT.split("\n").length - 1); // -1 for trailing newline
        expect(rebuilt!.fileDiff.deletionLines.length).toBe(OLD_CONTENT.split("\n").length - 1);
      }
    });
  });

  describe("isExpandable", () => {
    it("returns false for partial items", () => {
      const partial = makePartialItem();
      expect(isExpandable(partial)).toBe(false);
    });

    it("returns true for full-content items", () => {
      const partial = makePartialItem();
      const rebuilt = rebuildItemWithFullContent(
        partial,
        { name: "hello.ts", contents: OLD_CONTENT },
        { name: "hello.ts", contents: NEW_CONTENT },
      );

      expect(isExpandable(rebuilt!)).toBe(true);
    });

    it("returns false for file-type items", () => {
      const fileItem: CodeViewItem<string> = {
        id: "file:x.ts",
        type: "file",
        file: { name: "x.ts", contents: "hello" },
      };
      expect(isExpandable(fileItem)).toBe(false);
    });
  });
});
