import { beforeEach, describe, expect, it } from "vitest";
import {
  _resetForTests,
  addEditorFeedback,
  beginEditorFeedbackSend,
  clearEditorFeedback,
  editEditorFeedback,
  endEditorFeedbackSend,
  getEditorFeedback,
  getEditorFeedbackCount,
  isEditorFeedbackSending,
  removeEditorFeedback,
} from "../editor-feedback.svelte";

const snapshot = {
  filePath: "src/example.ts",
  startLine: 2,
  endLine: 2,
  language: "typescript",
  selectedText: "const answer = 42;",
  contextBefore: "export function answer() {",
  contextAfter: "}",
  isUnsaved: true,
};

describe("editor-feedback", () => {
  beforeEach(_resetForTests);

  it("stores immutable snapshots independently for each session", () => {
    const first = addEditorFeedback("s1", { ...snapshot, text: "Use a named constant." });
    addEditorFeedback("s2", { ...snapshot, text: "Other session." });

    expect(first.id).toBeTruthy();
    expect(first.createdAt).toBeGreaterThan(0);
    expect(getEditorFeedback("s1")).toEqual([first]);
    expect(getEditorFeedback("s2")).toHaveLength(1);
  });

  it("edits only the note text and preserves the captured source snapshot", () => {
    const feedback = addEditorFeedback("s1", { ...snapshot, text: "Original note." });
    editEditorFeedback("s1", feedback.id, "Updated note.");

    expect(getEditorFeedback("s1")[0]).toEqual({ ...feedback, text: "Updated note." });
  });

  it("locks feedback sending for the session until delivery finishes", () => {
    expect(beginEditorFeedbackSend("s1")).toBe(true);
    expect(isEditorFeedbackSending("s1")).toBe(true);
    expect(beginEditorFeedbackSend("s1")).toBe(false);

    endEditorFeedbackSend("s1");

    expect(isEditorFeedbackSending("s1")).toBe(false);
    expect(beginEditorFeedbackSend("s1")).toBe(true);
  });

  it("removes and clears only feedback for the requested session", () => {
    const first = addEditorFeedback("s1", { ...snapshot, text: "One." });
    addEditorFeedback("s1", { ...snapshot, text: "Two." });
    addEditorFeedback("s2", { ...snapshot, text: "Other." });

    removeEditorFeedback("s1", first.id);
    expect(getEditorFeedbackCount("s1")).toBe(1);
    clearEditorFeedback("s1");

    expect(getEditorFeedback("s1")).toEqual([]);
    expect(getEditorFeedbackCount("s2")).toBe(1);
  });
});
