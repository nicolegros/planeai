import type { ChangedFile } from "./types";

export type ReviewTreeRow = ReviewTreeFolderRow | ReviewTreeFileRow;

export interface ReviewTreeFolderRow {
  kind: "folder";
  path: string;
  name: string;
  depth: number;
}

export interface ReviewTreeFileRow {
  kind: "file";
  path: string;
  name: string;
  depth: number;
  fileIndex: number;
}

interface FolderNode {
  kind: "folder";
  path: string;
  name: string;
  children: TreeNode[];
  folders: Map<string, FolderNode>;
}

interface FileNode {
  kind: "file";
  path: string;
  name: string;
  fileIndex: number;
}

type TreeNode = FolderNode | FileNode;

function pathParts(path: string): string[] {
  return path.split("/").filter(Boolean);
}

function buildTree(files: readonly Pick<ChangedFile, "path">[]): FolderNode {
  const root: FolderNode = { kind: "folder", path: "", name: "", children: [], folders: new Map() };

  files.forEach((file, fileIndex) => {
    const parts = pathParts(file.path);
    if (parts.length === 0) return;

    let parent = root;
    let folderPath = "";
    for (const part of parts.slice(0, -1)) {
      folderPath = folderPath ? `${folderPath}/${part}` : part;
      let folder = parent.folders.get(part);
      if (!folder) {
        folder = { kind: "folder", path: folderPath, name: part, children: [], folders: new Map() };
        parent.folders.set(part, folder);
        parent.children.push(folder);
      }
      parent = folder;
    }

    parent.children.push({ kind: "file", path: file.path, name: parts.at(-1)!, fileIndex });
  });

  return root;
}

/** Returns every changed-file directory, preserving the order it first appears. */
export function getReviewFolderPaths(files: readonly Pick<ChangedFile, "path">[]): Set<string> {
  const folders = new Set<string>();
  for (const file of files) {
    let path = "";
    for (const part of pathParts(file.path).slice(0, -1)) {
      path = path ? `${path}/${part}` : part;
      folders.add(path);
    }
  }
  return folders;
}

/** Flattens the visible portion of the nested review tree for rendering and keyboard traversal. */
export function getReviewTreeRows(
  files: readonly Pick<ChangedFile, "path">[],
  expandedFolders: ReadonlySet<string>,
): ReviewTreeRow[] {
  const rows: ReviewTreeRow[] = [];

  function visit(nodes: readonly TreeNode[], depth: number) {
    for (const node of nodes) {
      if (node.kind === "file") {
        rows.push({ ...node, depth });
        continue;
      }

      rows.push({ kind: "folder", path: node.path, name: node.name, depth });
      if (expandedFolders.has(node.path)) visit(node.children, depth + 1);
    }
  }

  visit(buildTree(files).children, 0);
  return rows;
}

/**
 * Initializes all folders as expanded. On refresh, retain every existing choice
 * and expand folders that have just appeared in the diff.
 */
export function reconcileReviewTreeExpansion(
  previousExpandedFolders: ReadonlySet<string>,
  previousFolders: ReadonlySet<string>,
  currentFolders: ReadonlySet<string>,
  initialized: boolean,
): Set<string> {
  if (!initialized) return new Set(currentFolders);

  const expanded = new Set<string>();
  for (const folder of currentFolders) {
    if (!previousFolders.has(folder) || previousExpandedFolders.has(folder)) expanded.add(folder);
  }
  return expanded;
}

/** Reveals a selected file without changing the user's choice for unrelated folders. */
export function expandReviewTreeAncestors(
  expandedFolders: ReadonlySet<string>,
  filePath: string,
): Set<string> {
  const expanded = new Set(expandedFolders);
  let path = "";
  for (const part of pathParts(filePath).slice(0, -1)) {
    path = path ? `${path}/${part}` : part;
    expanded.add(path);
  }
  return expanded;
}
