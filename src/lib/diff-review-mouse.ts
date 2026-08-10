import type { SelectedLineRange } from "@pierre/diffs";

export type DraftNavigation = "same-file" | "change-file" | "reload";

export interface CommentLineTarget {
  startLine: number;
  endLine: number;
  type: "line" | "hunk";
}

/**
 * Convert a visual diff selection into the existing, side-agnostic review
 * comment model. The current keyboard workflow stores only line numbers, so
 * mouse selections deliberately retain that same behavior.
 */
export function commentTargetFromSelection(range: SelectedLineRange): CommentLineTarget {
  const startLine = Math.min(range.start, range.end);
  const endLine = Math.max(range.start, range.end);
  return { startLine, endLine, type: startLine === endLine ? "line" : "hunk" };
}

/**
 * A split diff has one active selection pane. Keep a drag anchored to its
 * origin pane when the pointer crosses the center gutter.
 */
export function lockSelectionToOriginSide(
  range: SelectedLineRange,
  originSide = range.side,
): SelectedLineRange {
  if (!originSide) return { start: range.start, end: range.end };
  return { start: range.start, end: range.end, side: originSide };
}

/**
 * Same-file line selection must not destroy an in-progress draft. File
 * changes and diff rebuilds need an explicit discard decision.
 */
export function shouldConfirmDraftDiscard(hasDraft: boolean, navigation: DraftNavigation): boolean {
  return hasDraft && navigation !== "same-file";
}

export function selectionForContextMenu(
  selectedRange: SelectedLineRange | null,
  clickedRange: SelectedLineRange,
): SelectedLineRange {
  if (
    selectedRange &&
    selectedRange.side === clickedRange.side &&
    clickedRange.start >= Math.min(selectedRange.start, selectedRange.end) &&
    clickedRange.start <= Math.max(selectedRange.start, selectedRange.end)
  ) {
    return selectedRange;
  }
  return clickedRange;
}

export function selectionLabel(range: SelectedLineRange): string {
  const { startLine, endLine } = commentTargetFromSelection(range);
  return startLine === endLine
    ? `Comment on line ${startLine}`
    : `Comment on lines ${startLine}–${endLine}`;
}
