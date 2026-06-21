import { describe, it, expect } from "vitest";
import { serializeComments } from "./review-serializer";
import type { ReviewComment } from "./review-comments.svelte";
import type { FileDiff } from "./types";

function makeComment(
  overrides: Partial<ReviewComment> & Pick<ReviewComment, "filePath" | "text">,
): ReviewComment {
  return {
    id: crypto.randomUUID(),
    type: "line",
    startLine: 1,
    endLine: 1,
    createdAt: Date.now(),
    ...overrides,
  };
}

describe("serializeComments", () => {
  it("returns empty string for no comments", () => {
    expect(serializeComments([], new Map())).toBe("");
  });

  it("serializes a line comment with code context", () => {
    const comments: ReviewComment[] = [
      makeComment({
        filePath: "src/lib/api.ts",
        type: "line",
        startLine: 3,
        endLine: 3,
        text: "Validate the id parameter.",
      }),
    ];
    const diffs = new Map<string, FileDiff>([
      [
        "src/lib/api.ts",
        {
          original: "",
          modified:
            "import { invoke } from './tauri';\n\nexport function fetchUser(id: string) {\n  return invoke('get_user', { id })\n}\n",
          language: "typescript",
        },
      ],
    ]);

    const result = serializeComments(comments, diffs);

    expect(result).toContain("Please address these review comments:");
    expect(result).toContain("--- src/lib/api.ts (line 3) ---");
    expect(result).toContain("```typescript");
    expect(result).toContain("export function fetchUser(id: string) {");
    expect(result).toContain("```");
    expect(result).toContain("Comment: Validate the id parameter.");
  });

  it("serializes a hunk comment with line range", () => {
    const comments: ReviewComment[] = [
      makeComment({
        filePath: "src/utils.ts",
        type: "hunk",
        startLine: 2,
        endLine: 4,
        text: "Extract this into a helper.",
      }),
    ];
    const diffs = new Map<string, FileDiff>([
      [
        "src/utils.ts",
        {
          original: "",
          modified:
            "const a = 1;\nconst b = 2;\nconst c = 3;\nconst d = 4;\nconst e = 5;\nconst f = 6;\n",
          language: "typescript",
        },
      ],
    ]);

    const result = serializeComments(comments, diffs);

    expect(result).toContain("--- src/utils.ts (lines 2-4) ---");
    expect(result).toContain("Comment: Extract this into a helper.");
  });

  it("serializes file-level comments without code context", () => {
    const comments: ReviewComment[] = [
      makeComment({
        filePath: "src/components/Terminal.svelte",
        type: "file",
        startLine: 0,
        endLine: 0,
        text: "Extract terminal setup into a reusable hook.",
      }),
    ];

    const result = serializeComments(comments, new Map());

    expect(result).toContain("--- src/components/Terminal.svelte (file-level) ---");
    expect(result).toContain("Comment: Extract terminal setup into a reusable hook.");
    expect(result).not.toContain("```");
  });

  it("groups comments by file and orders by line number", () => {
    const comments: ReviewComment[] = [
      makeComment({ filePath: "b.ts", startLine: 10, endLine: 10, text: "second" }),
      makeComment({ filePath: "a.ts", startLine: 5, endLine: 5, text: "a-file" }),
      makeComment({ filePath: "b.ts", startLine: 2, endLine: 2, text: "first" }),
    ];
    const diffs = new Map<string, FileDiff>([
      [
        "a.ts",
        { original: "", modified: "line1\nline2\nline3\nline4\nline5\n", language: "typescript" },
      ],
      [
        "b.ts",
        { original: "", modified: "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n", language: "typescript" },
      ],
    ]);

    const result = serializeComments(comments, diffs);

    const firstIdx = result.indexOf("Comment: first");
    const secondIdx = result.indexOf("Comment: second");
    // Within file b.ts, line 2 comment should come before line 10 comment
    expect(firstIdx).toBeLessThan(secondIdx);
  });

  it("ends with a newline", () => {
    const comments: ReviewComment[] = [
      makeComment({ filePath: "x.ts", type: "file", startLine: 0, endLine: 0, text: "hi" }),
    ];
    const result = serializeComments(comments, new Map());
    expect(result.endsWith("\n")).toBe(true);
  });
});
