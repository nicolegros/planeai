import { describe, expect, it } from "vitest";
import { serializeEditorFeedback } from "./editor-feedback-serializer";
import type { EditorFeedback } from "./editor-feedback.svelte";

const feedback: EditorFeedback = {
  id: "feedback-1",
  createdAt: 1,
  filePath: "src/example.ts",
  startLine: 3,
  endLine: 4,
  language: "typescript",
  selectedText: "const value = 1;\nreturn value;",
  contextBefore: "export function example() {\n  // setup",
  contextAfter: "}",
  isUnsaved: true,
  text: "Simplify this branch.",
};

describe("serializeEditorFeedback", () => {
  it("returns an empty prompt for an empty queue", () => {
    expect(serializeEditorFeedback([])).toBe("");
  });

  it("serializes exact snapshot content, context, note, and unsaved state", () => {
    const result = serializeEditorFeedback([feedback]);

    expect(result).toContain("Please address this editor feedback:");
    expect(result).toContain("--- src/example.ts (lines 3-4; unsaved editor content) ---");
    expect(result).toContain("Context before selection:");
    expect(result).toContain("Selected code:");
    expect(result).toContain("```typescript");
    expect(result).toContain("const value = 1;\nreturn value;");
    expect(result).toContain("Context after selection:");
    expect(result).toContain("Comment: Simplify this branch.");
    expect(result.endsWith("\n")).toBe(true);
  });

  it("omits unavailable surrounding context and labels a single saved line", () => {
    const result = serializeEditorFeedback([
      {
        ...feedback,
        startLine: 1,
        endLine: 1,
        contextBefore: "",
        contextAfter: "",
        isUnsaved: false,
      },
    ]);

    expect(result).toContain("--- src/example.ts (line 1) ---");
    expect(result).not.toContain("Context before selection:");
    expect(result).not.toContain("Context after selection:");
    expect(result).not.toContain("unsaved editor content");
  });
});
