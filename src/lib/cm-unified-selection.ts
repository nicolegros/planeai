import type { Text } from "@codemirror/state";
import type { ReviewSelection } from "./review-diff";

interface OriginalChunkRange {
  fromA: number;
  toA: number;
  endA: number;
}

/** Maps a rendered line in a unified deletion widget back to its original document line. */
export function originalSelectionForDeletedLine(
  original: Text,
  chunk: OriginalChunkRange,
  deletedLineOffset: number,
): ReviewSelection | null {
  if (!Number.isInteger(deletedLineOffset) || deletedLineOffset < 0 || chunk.fromA >= chunk.toA) return null;

  const from = Math.max(0, Math.min(chunk.fromA, original.length));
  const end = Math.max(from, Math.min(chunk.endA, original.length));
  const firstLine = original.lineAt(from).number;
  const lastLine = original.lineAt(end).number;
  const line = firstLine + deletedLineOffset;

  return line <= lastLine ? { side: "original", startLine: line, endLine: line } : null;
}
