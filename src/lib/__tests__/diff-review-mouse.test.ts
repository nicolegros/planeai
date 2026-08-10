import { describe, expect, it } from "vitest";
import {
  commentTargetFromSelection,
  lockSelectionToOriginSide,
  selectionForContextMenu,
  selectionLabel,
  shouldConfirmDraftDiscard,
} from "../diff-review-mouse";

describe("diff review mouse interaction policy", () => {
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

  it("locks split-diff drags to their origin side after crossing the gutter", () => {
    expect(
      lockSelectionToOriginSide({ start: 4, end: 9, side: "additions", endSide: "deletions" }),
    ).toEqual({
      start: 4,
      end: 9,
      side: "additions",
    });
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
