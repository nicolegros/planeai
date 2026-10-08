import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  MAX_EDITOR_FEEDBACK_BYTES,
  createEditorFeedbackSnapshot,
} from "./editor-feedback-selection";

function snapshot(doc: Text, from: number, to: number) {
  return createEditorFeedbackSnapshot({
    doc,
    from,
    to,
    filePath: "src/example.ts",
    language: "typescript",
    isUnsaved: true,
  });
}

describe("createEditorFeedbackSnapshot", () => {
  it("captures exact selected text, inclusive lines, and two lines of context", () => {
    const doc = Text.of(["one", "two", "three", "four", "five", "six"]);
    const result = snapshot(doc, doc.line(3).from, doc.line(4).to);

    expect("snapshot" in result).toBe(true);
    if (!("snapshot" in result)) return;
    expect(result.snapshot).toMatchObject({
      filePath: "src/example.ts",
      startLine: 3,
      endLine: 4,
      selectedText: "three\nfour",
      contextBefore: "one\ntwo",
      contextAfter: "five\nsix",
      isUnsaved: true,
    });
  });

  it("does not count a trailing selected newline as the next line", () => {
    const doc = Text.of(["one", "two", "three"]);
    const result = snapshot(doc, doc.line(2).from, doc.line(3).from);

    expect("snapshot" in result && result.snapshot.endLine).toBe(2);
  });

  it("rejects empty and oversized selections without truncating", () => {
    const doc = Text.of(["small"]);
    expect(snapshot(doc, 0, 0)).toEqual({ error: "empty" });

    const large = Text.of(["x".repeat(MAX_EDITOR_FEEDBACK_BYTES + 1)]);
    expect(snapshot(large, 0, large.length)).toEqual({ error: "too-large" });
  });

  it("bounds context around a small selection beside an oversized line", () => {
    const doc = Text.of(["x".repeat(MAX_EDITOR_FEEDBACK_BYTES * 2), "selected"]);
    const result = snapshot(doc, doc.line(2).from, doc.line(2).to);

    expect("snapshot" in result).toBe(true);
    if (!("snapshot" in result)) return;
    expect(new TextEncoder().encode(result.snapshot.contextBefore).byteLength).toBeLessThanOrEqual(
      4096,
    );
  });
});
