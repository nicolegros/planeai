import type { SelectedLineRange } from "@pierre/diffs";

export type DraftNavigation = "same-file" | "change-file" | "reload";

export interface PointerSelectionInput {
  primaryButton: boolean;
  isCodeLine: boolean;
  altKey: boolean;
}

/**
 * Code-body drags need application-managed selection because @pierre/diffs
 * reserves its built-in line-selection session for line-number gutters.
 */
export function pointerSelectionMode({
  primaryButton,
  isCodeLine,
  altKey,
}: PointerSelectionInput): "line" | "text" | "ignore" {
  if (!primaryButton || !isCodeLine) return "ignore";
  return altKey ? "text" : "line";
}

/** Build the current visual range from the pointer-down anchor and drag endpoint. */
export function buildPointerSelectionRange(
  anchor: SelectedLineRange,
  endpoint: SelectedLineRange,
): SelectedLineRange {
  return {
    start: anchor.start,
    end: endpoint.end,
    ...(anchor.side ? { side: anchor.side } : {}),
    ...(anchor.side !== endpoint.side && endpoint.side ? { endSide: endpoint.side } : {}),
  };
}

export interface ClickSelectionInput {
  isInteractive: boolean;
  preserveCompletedBodyDrag: boolean;
}

/** A completed body drag owns its trailing click; blank-space clicks still clear selection. */
export function shouldClearSelectionAfterClick({
  isInteractive,
  preserveCompletedBodyDrag,
}: ClickSelectionInput): boolean {
  return !isInteractive && !preserveCompletedBodyDrag;
}

export interface GutterActionRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** Place the action on the lower visual endpoint, regardless of drag direction. */
export function gutterActionAnchor(
  start: GutterActionRect,
  end: GutterActionRect,
): Pick<GutterActionRect, "left" | "top"> {
  const endpoint = start.top >= end.top ? start : end;
  return { left: endpoint.right, top: endpoint.top };
}

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

/** Compare an existing comment with a visual selection after normalizing drag direction. */
export function commentRangeOverlapsSelection(
  comment: Pick<CommentLineTarget, "startLine" | "endLine">,
  selection: SelectedLineRange,
): boolean {
  const target = commentTargetFromSelection(selection);
  return comment.startLine <= target.endLine && comment.endLine >= target.startLine;
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
