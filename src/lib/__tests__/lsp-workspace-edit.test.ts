import { describe, expect, it } from "vitest";
import { textEditsByUri } from "../lsp-workspace-edit";

const edit = (line: number, newText: string) => ({
  range: {
    start: { line, character: 0 },
    end: { line, character: 1 },
  },
  newText,
});

describe("textEditsByUri", () => {
  it("normalizes rust-analyzer documentChanges edits", () => {
    const edits = textEditsByUri({
      documentChanges: [
        {
          textDocument: { uri: "file:///repo/main.rs" },
          edits: [edit(2, "renamed")],
        },
      ],
    });

    expect(edits.get("file:///repo/main.rs")).toEqual([edit(2, "renamed")]);
  });

  it("merges legacy changes with text document edits and ignores resource operations", () => {
    const edits = textEditsByUri({
      changes: { "file:///repo/main.rs": [edit(0, "legacy")] },
      documentChanges: [
        { textDocument: { uri: "file:///repo/main.rs" }, edits: [edit(1, "modern")] },
        {},
      ],
    });

    expect(edits.get("file:///repo/main.rs")).toEqual([edit(0, "legacy"), edit(1, "modern")]);
  });
});
