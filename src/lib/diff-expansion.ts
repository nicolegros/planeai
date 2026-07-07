/**
 * Utilities for expanding partial diff items to full file content.
 * The @pierre/diffs library only allows hunk expansion when isPartial is false,
 * meaning we need full file content in additionLines/deletionLines.
 */
import { parseDiffFromFile, type CodeViewItem, type FileContents } from "@pierre/diffs";

/**
 * Rebuild a patch-based (partial) diff item using full file contents.
 * Returns a new CodeViewItem with isPartial:false that supports native hunk expansion.
 * Preserves any existing annotations from the original item.
 */
export function rebuildItemWithFullContent<T>(
  item: CodeViewItem<T>,
  oldFile: FileContents,
  newFile: FileContents,
): CodeViewItem<T> | null {
  if (item.type !== "diff") return null;

  const fullDiff = parseDiffFromFile(oldFile, newFile);
  // Let the library detect language from filename
  fullDiff.lang = undefined;

  return {
    id: item.id,
    type: "diff",
    fileDiff: fullDiff,
    annotations: item.annotations,
    version: (item.version ?? 0) + 1,
    collapsed: item.collapsed,
  };
}

/**
 * Check if a diff item supports hunk expansion (has full file content).
 */
export function isExpandable<T>(item: CodeViewItem<T>): boolean {
  return item.type === "diff" && !item.fileDiff.isPartial;
}
