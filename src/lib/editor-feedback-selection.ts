import type { Text } from "@codemirror/state";
import type { EditorFeedbackSnapshot } from "./editor-feedback.svelte";

export const MAX_EDITOR_FEEDBACK_LINES = 200;
export const MAX_EDITOR_FEEDBACK_BYTES = 12 * 1024;

export type EditorFeedbackSelectionResult =
  | { snapshot: EditorFeedbackSnapshot }
  | { error: "empty" | "too-large" };

interface SelectionSnapshotInput {
  doc: Text;
  from: number;
  to: number;
  filePath: string;
  language: string;
  isUnsaved: boolean;
}

export function createEditorFeedbackSnapshot({
  doc,
  from,
  to,
  filePath,
  language,
  isUnsaved,
}: SelectionSnapshotInput): EditorFeedbackSelectionResult {
  if (from === to) return { error: "empty" };

  const selectedText = doc.sliceString(from, to);
  const endPosition = selectedText.endsWith("\n") ? to - 1 : to;
  const startLine = doc.lineAt(from).number;
  const endLine = doc.lineAt(endPosition).number;
  const lineCount = endLine - startLine + 1;
  const byteCount = new TextEncoder().encode(selectedText).byteLength;
  if (lineCount > MAX_EDITOR_FEEDBACK_LINES || byteCount > MAX_EDITOR_FEEDBACK_BYTES) {
    return { error: "too-large" };
  }

  const contextBefore = linesInRange(doc, Math.max(1, startLine - 2), startLine - 1);
  const contextAfter = linesInRange(doc, endLine + 1, Math.min(doc.lines, endLine + 2));
  return {
    snapshot: {
      filePath,
      startLine,
      endLine,
      language,
      selectedText,
      contextBefore,
      contextAfter,
      isUnsaved,
    },
  };
}

function linesInRange(doc: Text, startLine: number, endLine: number): string {
  if (startLine > endLine) return "";
  const lines: string[] = [];
  for (let line = startLine; line <= endLine; line++) lines.push(doc.line(line).text);
  return lines.join("\n");
}
