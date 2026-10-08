import type { ChangedFile } from "./types";

/** Keep the currently selected path selected across a refreshed diff when possible. */
export function getRefreshedSelectedIndex(
  currentFiles: readonly Pick<ChangedFile, "path">[],
  selectedIndex: number,
  refreshedFiles: readonly Pick<ChangedFile, "path">[],
): number {
  const selectedPath = currentFiles[selectedIndex]?.path;
  const refreshedSelectedIndex = selectedPath
    ? refreshedFiles.findIndex((file) => file.path === selectedPath)
    : -1;
  return refreshedSelectedIndex >= 0
    ? refreshedSelectedIndex
    : Math.min(selectedIndex, Math.max(refreshedFiles.length - 1, 0));
}
