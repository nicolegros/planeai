import { describe, expect, it } from "vitest";
import {
  buildPointerSelectionRange,
  commentRangeOverlapsSelection,
  commentTargetFromSelection,
  gutterActionAnchor,
  lockSelectionToOriginSide,
  pointerSelectionMode,
  selectionForContextMenu,
  selectionLabel,
  shouldClearSelectionAfterClick,
  shouldConfirmDraftDiscard,
} from "../diff-review-mouse";

describe("diff review mouse interaction policy", () => {
  it("extends code-body drags from their initial line instead of replacing them with each endpoint", () => {
    expect(
      buildPointerSelectionRange(
        { start: 4, end: 4, side: "additions" },
        { start: 9, end: 9, side: "additions" },
      ),
    ).toEqual({ start: 4, end: 9, side: "additions" });
    expect(
      buildPointerSelectionRange(
        { start: 10, end: 10, side: "deletions" },
        { start: 7, end: 7, side: "additions" },
      ),
    ).toEqual({ start: 10, end: 7, side: "deletions", endSide: "additions" });
  });

  it("preserves a completed body-drag selection against its trailing click", () => {
    expect(
      shouldClearSelectionAfterClick({ isInteractive: false, preserveCompletedBodyDrag: true }),
    ).toBe(false);
    expect(
      shouldClearSelectionAfterClick({ isInteractive: false, preserveCompletedBodyDrag: false }),
    ).toBe(true);
    expect(
      shouldClearSelectionAfterClick({ isInteractive: true, preserveCompletedBodyDrag: false }),
    ).toBe(false);
  });

  it("uses visual line selection for ordinary code-body drags but preserves Option-drag text selection", () => {
    expect(pointerSelectionMode({ primaryButton: true, isCodeLine: true, altKey: false })).toBe(
      "line",
    );
    expect(pointerSelectionMode({ primaryButton: true, isCodeLine: true, altKey: true })).toBe(
      "text",
    );
    expect(pointerSelectionMode({ primaryButton: true, isCodeLine: false, altKey: false })).toBe(
      "ignore",
    );
  });

  it("uses the existing side-agnostic line comment model for a selected range", () => {
    expect(commentTargetFromSelection({ start: 12, end: 8, side: "deletions" })).toEqual({
      startLine: 8,
      endLine: 12,
      type: "hunk",
    });
    expect(commentTargetFromSelection({ start: 4, end: 4, side: "additions" })).toEqual({
      startLine: 4,
      endLine: 4,
      type: "line",
    });
  });

  it("detects comment overlap for selected ranges in either drag direction", () => {
    expect(
      commentRangeOverlapsSelection(
        { startLine: 8, endLine: 10 },
        { start: 12, end: 9, side: "additions" },
      ),
    ).toBe(true);
    expect(
      commentRangeOverlapsSelection(
        { startLine: 1, endLine: 4 },
        { start: 12, end: 9, side: "additions" },
      ),
    ).toBe(false);
  });

  it("locks split-diff drags to their origin side after crossing the gutter", () => {
    expect(
      lockSelectionToOriginSide({ start: 4, end: 9, side: "additions", endSide: "deletions" }),
    ).toEqual({
      start: 4,
      end: 9,
      side: "additions",
    });
  });

  it("anchors the gutter action to the lower visual endpoint for either drag direction", () => {
    expect(
      gutterActionAnchor(
        { left: 40, right: 64, top: 80, bottom: 100 },
        { left: 40, right: 64, top: 140, bottom: 160 },
      ),
    ).toEqual({ left: 64, top: 140 });
    expect(
      gutterActionAnchor(
        { left: 40, right: 64, top: 140, bottom: 160 },
        { left: 40, right: 64, top: 80, bottom: 100 },
      ),
    ).toEqual({ left: 64, top: 140 });
  });

  it("preserves same-file drafts but requires confirmation before changing files or rebuilding", () => {
    expect(shouldConfirmDraftDiscard(true, "same-file")).toBe(false);
    expect(shouldConfirmDraftDiscard(true, "change-file")).toBe(true);
    expect(shouldConfirmDraftDiscard(true, "reload")).toBe(true);
    expect(shouldConfirmDraftDiscard(false, "reload")).toBe(false);
  });

  it("labels the inline gutter action with the exact selected range", () => {
    expect(selectionLabel({ start: 9, end: 9, side: "additions" })).toBe("Comment on line 9");
    expect(selectionLabel({ start: 14, end: 11, side: "additions" })).toBe(
      "Comment on lines 11–14",
    );
  });

  it("keeps a selected range when its code context menu is opened from within that range", () => {
    const selected = { start: 8, end: 12, side: "additions" as const };
    expect(selectionForContextMenu(selected, { start: 10, end: 10, side: "additions" })).toEqual(
      selected,
    );
    expect(selectionForContextMenu(selected, { start: 14, end: 14, side: "additions" })).toEqual({
      start: 14,
      end: 14,
      side: "additions",
    });
  });
});
