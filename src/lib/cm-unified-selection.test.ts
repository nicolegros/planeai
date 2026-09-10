import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { originalSelectionForDeletedLine } from "./cm-unified-selection";

describe("originalSelectionForDeletedLine", () => {
  const original = Text.of(["keep", "removed one", "removed two", "tail"]);
  const deletedChunk = {
    fromA: original.line(2).from,
    toA: original.line(4).from,
    endA: original.line(3).to,
  };

  it("maps a unified deleted-widget line to its original document line", () => {
    expect(originalSelectionForDeletedLine(original, deletedChunk, 1)).toEqual({
      side: "original",
      startLine: 3,
      endLine: 3,
    });
  });

  it("does not produce selections outside the original changed range", () => {
    expect(originalSelectionForDeletedLine(original, deletedChunk, 2)).toBeNull();
  });
});
