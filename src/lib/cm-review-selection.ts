import type { Text } from "@codemirror/state";
import type { ReviewSelection } from "./review-diff";

/** Converts a CodeMirror document range to inclusive review line numbers. */
export function reviewSelectionForDocumentRange(
  document: Text,
  side: ReviewSelection["side"],
  from: number,
  to: number,
): ReviewSelection {
  const clampedFrom = Math.max(0, Math.min(from, document.length));
  const clampedTo = Math.max(0, Math.min(to, document.length));
  const start = Math.min(clampedFrom, clampedTo);
  const end = Math.max(clampedFrom, clampedTo);
  // CodeMirror line selections include the trailing newline. Review selections
  // are inclusive line ranges, so the newline belongs to the preceding line.
  // A terminal empty line is different: document.length is that line's position
  // and an inclusive visual range ending there must retain it.
  const terminalLine = document.line(document.lines);
  const includesTerminalEmptyLine =
    end === document.length && terminalLine.from === document.length;
  const lastPosition = start === end || includesTerminalEmptyLine ? end : Math.max(start, end - 1);

  return {
    side,
    startLine: document.lineAt(start).number,
    endLine: document.lineAt(lastPosition).number,
  };
}

/** Builds a CodeMirror range that contains every line between two visual line endpoints. */
export function documentRangeForVisualLines(
  document: Text,
  anchorLine: number,
  headLine: number,
): { anchor: number; head: number } {
  const clampLine = (line: number) => Math.max(1, Math.min(line, document.lines));
  const anchor = clampLine(anchorLine);
  const head = clampLine(headLine);
  const first = Math.min(anchor, head);
  const last = Math.max(anchor, head);
  const from = document.line(first).from;
  const to = last === document.lines ? document.length : document.line(last + 1).from;

  return anchor <= head
    ? { anchor: document.line(anchor).from, head: to }
    : { anchor: to, head: from };
}
