import type { Text } from "@codemirror/state";
import type { EditorFeedbackSnapshot } from "./editor-feedback.svelte";

export const MAX_EDITOR_FEEDBACK_LINES = 200;
export const MAX_EDITOR_FEEDBACK_BYTES = 12 * 1024;
export const MAX_EDITOR_FEEDBACK_CONTEXT_BYTES = 4 * 1024;

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

  const contextBefore = linesInRange(
    doc,
    Math.max(1, startLine - 2),
    startLine - 1,
    MAX_EDITOR_FEEDBACK_CONTEXT_BYTES,
  );
  const contextAfter = linesInRange(
    doc,
    endLine + 1,
    Math.min(doc.lines, endLine + 2),
    MAX_EDITOR_FEEDBACK_CONTEXT_BYTES - utf8ByteLength(contextBefore),
  );
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

function linesInRange(doc: Text, startLine: number, endLine: number, maxBytes: number): string {
  if (startLine > endLine || maxBytes <= 0) return "";
  const lines: string[] = [];
  let remainingBytes = maxBytes;
  for (let line = startLine; line <= endLine; line++) {
    const separator = lines.length > 0 ? "\n" : "";
    const separatorBytes = separator ? 1 : 0;
    if (separatorBytes >= remainingBytes) break;
    const text = truncateUtf8(doc.line(line).text, remainingBytes - separatorBytes);
    lines.push(text);
    remainingBytes -= separatorBytes + utf8ByteLength(text);
    if (text.length < doc.line(line).text.length) break;
  }
  return lines.join("\n");
}

function truncateUtf8(text: string, maxBytes: number): string {
  let bytes = 0;
  let end = 0;
  for (; end < text.length;) {
    const codePoint = text.codePointAt(end)!;
    const codePointBytes =
      codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
    if (bytes + codePointBytes > maxBytes) break;
    bytes += codePointBytes;
    end += codePoint > 0xffff ? 2 : 1;
  }
  if (end === text.length) return text;
  const ellipsis = "…";
  if (maxBytes < utf8ByteLength(ellipsis)) return "";
  let truncated = text.slice(0, end);
  while (utf8ByteLength(truncated) + utf8ByteLength(ellipsis) > maxBytes) {
    const lastUnit = truncated.charCodeAt(truncated.length - 1);
    truncated = truncated.slice(
      0,
      truncated.length - (lastUnit >= 0xdc00 && lastUnit <= 0xdfff ? 2 : 1),
    );
  }
  return truncated + ellipsis;
}

function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}
