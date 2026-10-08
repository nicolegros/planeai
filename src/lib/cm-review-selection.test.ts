import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  documentRangeForVisualLines,
  reviewSelectionForDocumentRange,
} from "./cm-review-selection";

describe("reviewSelectionForDocumentRange", () => {
  const document = Text.of(["first", "second", "third"]);

  it("keeps a CodeMirror line selection's trailing newline on the selected line", () => {
    const first = document.line(1);
    expect(reviewSelectionForDocumentRange(document, "modified", first.from, first.to + 1)).toEqual(
      {
        side: "modified",
        startLine: 1,
        endLine: 1,
      },
    );
  });

  it("preserves inclusive multi-line ranges", () => {
    const first = document.line(1);
    const second = document.line(2);
    expect(
      reviewSelectionForDocumentRange(document, "modified", first.from, second.to + 1),
    ).toEqual({
      side: "modified",
      startLine: 1,
      endLine: 2,
    });
  });

  it("retains a terminal empty line in a visual selection", () => {
    const trailingNewlineDocument = Text.of(["first", ""]);
    const range = documentRangeForVisualLines(trailingNewlineDocument, 1, 2);
    expect(
      reviewSelectionForDocumentRange(
        trailingNewlineDocument,
        "modified",
        range.anchor,
        range.head,
      ),
    ).toEqual({
      side: "modified",
      startLine: 1,
      endLine: 2,
    });
  });

  it("keeps an empty cursor selection on its current line", () => {
    const second = document.line(2);
    expect(reviewSelectionForDocumentRange(document, "original", second.from, second.from)).toEqual(
      {
        side: "original",
        startLine: 2,
        endLine: 2,
      },
    );
  });

  it("keeps both endpoints when visual selection moves upward", () => {
    const range = documentRangeForVisualLines(document, 3, 2);
    expect(reviewSelectionForDocumentRange(document, "modified", range.anchor, range.head)).toEqual(
      {
        side: "modified",
        startLine: 2,
        endLine: 3,
      },
    );
  });

  it("keeps the original line when visual selection moves to the document start", () => {
    const range = documentRangeForVisualLines(document, 3, 1);
    expect(reviewSelectionForDocumentRange(document, "modified", range.anchor, range.head)).toEqual(
      {
        side: "modified",
        startLine: 1,
        endLine: 3,
      },
    );
  });
});
