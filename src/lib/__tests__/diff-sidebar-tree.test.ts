import { describe, expect, it } from "vitest";
import type { ChangedFile } from "../types";
import {
  expandReviewTreeAncestors,
  getReviewFolderPaths,
  getReviewTreeRows,
  reconcileReviewTreeExpansion,
} from "../diff-sidebar-tree";

const files: ChangedFile[] = [
  { path: "README.md", status: "M", additions: 1, deletions: 0, old_path: null },
  { path: "src/App.svelte", status: "M", additions: 2, deletions: 1, old_path: null },
  { path: "src/lib/api.ts", status: "A", additions: 3, deletions: 0, old_path: null },
  { path: "tests/api.test.ts", status: "D", additions: 0, deletions: 4, old_path: null },
];

describe("diff sidebar tree", () => {
  it("builds visible nested rows while preserving source file indexes", () => {
    const rows = getReviewTreeRows(files, new Set(["src", "src/lib", "tests"]));

    expect(rows).toEqual([
      { kind: "file", path: "README.md", name: "README.md", depth: 0, fileIndex: 0 },
      { kind: "folder", path: "src", name: "src", depth: 0 },
      { kind: "file", path: "src/App.svelte", name: "App.svelte", depth: 1, fileIndex: 1 },
      { kind: "folder", path: "src/lib", name: "lib", depth: 1 },
      { kind: "file", path: "src/lib/api.ts", name: "api.ts", depth: 2, fileIndex: 2 },
      { kind: "folder", path: "tests", name: "tests", depth: 0 },
      { kind: "file", path: "tests/api.test.ts", name: "api.test.ts", depth: 1, fileIndex: 3 },
    ]);
  });

  it("hides a collapsed folder's descendants but retains the folder row", () => {
    const rows = getReviewTreeRows(files, new Set(["tests"]));

    expect(rows.map((row) => row.path)).toEqual(["README.md", "src", "tests", "tests/api.test.ts"]);
  });

  it("initially expands folders, preserves existing choices, and expands new folders", () => {
    const initialFolders = getReviewFolderPaths(files);
    const initiallyExpanded = reconcileReviewTreeExpansion(
      new Set(),
      new Set(),
      initialFolders,
      false,
    );
    expect(initiallyExpanded).toEqual(initialFolders);

    const refreshedFiles = [
      ...files,
      { path: "docs/guide.md", status: "A", additions: 1, deletions: 0, old_path: null },
    ];
    const refreshedFolders = getReviewFolderPaths(refreshedFiles);
    const refreshed = reconcileReviewTreeExpansion(
      new Set(["src/lib", "tests"]),
      initialFolders,
      refreshedFolders,
      true,
    );

    expect(refreshed).toEqual(new Set(["src/lib", "tests", "docs"]));
  });

  it("expands every selected file ancestor without reopening unrelated folders", () => {
    expect(expandReviewTreeAncestors(new Set(["tests"]), "src/lib/api.ts")).toEqual(
      new Set(["tests", "src", "src/lib"]),
    );
  });
});
