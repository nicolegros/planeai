<script lang="ts">
  import { FileTree, prepareFileTreeInput, type FileTreeDropResult, type FileTreeRenameEvent, type GitStatusEntry } from "@pierre/trees";
  import { fileExplorer } from "../lib/api";
  import type { FsChangeEvent } from "../lib/types";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { getSettings } from "../lib/settings.svelte";

  interface Props {
    rootPath: string;
    sessionId: string;
    visible: boolean;
    activeFilePath?: string | null;
    modifiedPaths?: Set<string>;
    onOpenFile: (relativePath: string) => void;
    onPinFile: (relativePath: string) => void;
    onFocus: () => void;
  }

  let {
    rootPath,
    sessionId,
    visible,
    activeFilePath = null,
    modifiedPaths = new Set(),
    onOpenFile,
    onPinFile,
    onFocus,
  }: Props = $props();

  let panelWidth = $state(getLayoutWidth("file-explorer", 220));
  let treeContainer = $state<HTMLDivElement>();
  let fileTree = $state<FileTree | null>(null);
  let allPaths = $state<string[]>([]);
  let contextMenu = $state<{ x: number; y: number; path: string; isDir: boolean } | null>(null);
  let unlisten: (() => void) | null = null;
  let panelEl = $state<HTMLElement>();

  // --- Tree lifecycle ---

  async function loadAllPaths(): Promise<string[]> {
    return fileExplorer.listAllPaths(rootPath);
  }

  function createTree(paths: string[]) {
    if (fileTree) {
      fileTree.cleanUp();
    }

    const preparedInput = prepareFileTreeInput(paths);

    fileTree = new FileTree({
      preparedInput,
      search: true,
      flattenEmptyDirectories: true,
      density: "compact",
      icons: "standard",
      initialExpansion: "closed",
      onSelectionChange: handleSelectionChange,
      renaming: {
        onRename: handleRename,
      },
      dragAndDrop: {
        onDropComplete: handleDrop,
      },
    });

    if (treeContainer) {
      fileTree.render({ fileTreeContainer: treeContainer });
    }
  }

  function mountTree() {
    if (fileTree && treeContainer) {
      fileTree.render({ fileTreeContainer: treeContainer });
    }
  }

  // --- Event handlers ---

  function handleSelectionChange(selectedPaths: readonly string[]) {
    for (const path of selectedPaths) {
      // Check if it's a directory by trying to get the item
      const item = fileTree?.getItem(path);
      if (item && "expand" in item) {
        // It's a directory — toggle it
        item.toggle();
      } else {
        // It's a file — open it
        onOpenFile(path);
      }
    }
  }

  async function handleRename(event: FileTreeRenameEvent) {
    const oldAbsPath = rootPath + "/" + event.sourcePath;
    const newAbsPath = rootPath + "/" + event.destinationPath;
    await fileExplorer.rename(oldAbsPath, newAbsPath);
  }

  async function handleDrop(event: FileTreeDropResult) {
    for (const sourcePath of event.draggedPaths) {
      const sourceAbs = rootPath + "/" + sourcePath;
      const targetDir = event.target.directoryPath ?? "";
      const targetDirAbs = rootPath + "/" + targetDir;
      const fileName = sourcePath.split("/").pop() ?? sourcePath;
      const destAbs = targetDirAbs + "/" + fileName;
      await fileExplorer.rename(sourceAbs, destAbs);
    }
  }

  // --- Vim keyboard navigation ---

  function handleKeydown(e: KeyboardEvent) {
    if (!fileTree) return;
    const vimMode = getSettings().vim_mode ?? true;

    if (!vimMode) return; // Let the tree handle its own keyboard nav

    const key = e.key;
    if (key === "j") {
      e.preventDefault();
      e.stopPropagation();
      fileTree.focusNextItem();
    } else if (key === "k") {
      e.preventDefault();
      e.stopPropagation();
      fileTree.focusPreviousItem();
    } else if (key === "l") {
      e.preventDefault();
      e.stopPropagation();
      const focused = fileTree.getFocusedItem();
      if (focused && "expand" in focused) {
        focused.expand();
      }
    } else if (key === "h") {
      e.preventDefault();
      e.stopPropagation();
      const focused = fileTree.getFocusedItem();
      if (focused && "collapse" in focused) {
        focused.collapse();
      } else {
        fileTree.focusParentItem();
      }
    } else if (key === "g" && !e.ctrlKey) {
      e.preventDefault();
      e.stopPropagation();
      fileTree.focusFirstItem();
    } else if (key === "G") {
      e.preventDefault();
      e.stopPropagation();
      fileTree.focusLastItem();
    } else if (key === "/") {
      e.preventDefault();
      e.stopPropagation();
      fileTree.openSearch();
    }
  }

  // --- Double-click to pin ---

  function handleDblClick(_e: MouseEvent) {
    if (!fileTree) return;
    const selectedPaths = fileTree.getSelectedPaths();
    if (selectedPaths.length === 1) {
      const item = fileTree.getItem(selectedPaths[0]);
      if (item && !("expand" in item)) {
        onPinFile(selectedPaths[0]);
      }
    }
  }

  // --- Context menu (option C: outside the tree) ---

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    if (!fileTree) return;
    const focusedPath = fileTree.getFocusedPath();
    if (!focusedPath) return;

    const item = fileTree.getItem(focusedPath);
    const isDir = item ? "expand" in item : false;
    contextMenu = { x: e.clientX, y: e.clientY, path: focusedPath, isDir };
  }

  function contextMenuItems() {
    if (!contextMenu) return [];
    const items = [];
    if (contextMenu.isDir) {
      items.push({
        label: "New file",
        onSelect: () => createInDir(contextMenu!.path, false),
      });
      items.push({
        label: "New folder",
        onSelect: () => createInDir(contextMenu!.path, true),
      });
    }
    items.push({
      label: "Rename",
      onSelect: () => {
        if (fileTree && contextMenu) {
          fileTree.startRenaming(contextMenu.path);
        }
      },
    });
    items.push({
      label: "Delete",
      danger: true,
      onSelect: () => deleteEntry(contextMenu!.path),
    });
    return items;
  }

  async function createInDir(dirPath: string, isDir: boolean) {
    // Use a prompt approach — create with a default name then rename
    const absDir = rootPath + "/" + dirPath;
    const name = isDir ? "new-folder" : "new-file";
    const fullPath = absDir + "/" + name;
    if (isDir) {
      await fileExplorer.createDir(fullPath);
    } else {
      await fileExplorer.createFile(fullPath);
    }
    // Add to tree and start renaming
    const relativePath = dirPath + "/" + name;
    fileTree?.add(relativePath);
    // Expand parent if not already
    const dirItem = fileTree?.getItem(dirPath);
    if (dirItem && "expand" in dirItem) {
      dirItem.expand();
    }
    // Start renaming the new item
    fileTree?.startRenaming(relativePath);
  }

  async function deleteEntry(path: string) {
    const absPath = rootPath + "/" + path;
    await fileExplorer.deleteToTrash(absPath);
    fileTree?.remove(path);
  }

  // --- Git status sync ---

  $effect(() => {
    if (!fileTree) return;
    if (modifiedPaths.size === 0) {
      fileTree.setGitStatus(undefined);
      return;
    }
    const entries: GitStatusEntry[] = [];
    for (const p of modifiedPaths) {
      entries.push({ path: p, status: "modified" });
    }
    fileTree.setGitStatus(entries);
  });

  // --- Active file highlight via selection ---

  $effect(() => {
    if (!fileTree || !activeFilePath) return;
    fileTree.scrollToPath(activeFilePath, { offset: "nearest", focus: false });
  });

  // --- Filesystem watcher integration ---

  async function setupWatcher() {
    await fileExplorer.watch(sessionId, rootPath);
    const unlistenFn = await listen<FsChangeEvent>("fs-change", (event) => {
      if (event.payload.session_id !== sessionId) return;
      if (!fileTree) return;

      const absPath = event.payload.path;
      // Convert absolute path to relative
      let relPath = absPath;
      if (absPath.startsWith(rootPath)) {
        relPath = absPath.slice(rootPath.length);
        if (relPath.startsWith("/")) relPath = relPath.slice(1);
      }

      if (!relPath) return;

      switch (event.payload.kind) {
        case "create":
          fileTree.add(relPath);
          break;
        case "remove":
          fileTree.remove(relPath);
          break;
        case "rename":
          // Rename events on macOS come as two events (old path removed, new path created)
          // The notify crate may send the path for both — treat as create if exists
          fileTree.add(relPath);
          break;
        case "modify":
          // No structural tree change needed for modifications
          break;
      }
    });
    unlisten = unlistenFn;
  }

  async function teardownWatcher() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    await fileExplorer.unwatch(sessionId).catch(() => {});
  }

  // --- Lifecycle ---

  onMount(async () => {
    if (visible && rootPath) {
      allPaths = await loadAllPaths();
      createTree(allPaths);
      await setupWatcher();
    }
  });

  onDestroy(async () => {
    await teardownWatcher();
    fileTree?.cleanUp();
  });

  $effect(() => {
    if (visible && rootPath && !fileTree) {
      loadAllPaths().then((paths) => {
        allPaths = paths;
        createTree(paths);
        setupWatcher();
      });
    }
  });

  // Mount tree when container becomes available
  $effect(() => {
    if (treeContainer && fileTree) {
      mountTree();
    }
  });

  $effect(() => {
    if (visible && panelEl) {
      requestAnimationFrame(() => panelEl!.focus());
    }
  });
</script>

{#if visible}
<div
  bind:this={panelEl}
  tabindex="-1"
  role="toolbar"
  aria-label="File explorer"
  class="relative shrink-0 flex flex-col border-l border-border bg-canvas overflow-hidden outline-none"
  style:width="{panelWidth}px"
  onclick={onFocus}
  onkeydown={handleKeydown}
>
  <ResizeHandle side="left" bind:width={panelWidth} min={140} max={500} defaultWidth={220} onResizeEnd={(w) => setLayoutWidth("file-explorer", w)} />

  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-2 border-b border-border">
    <span class="text-xs font-semibold text-t2 uppercase tracking-wider">Files</span>
  </div>

  <!-- Tree (rendered by @pierre/trees) -->
  <div
    bind:this={treeContainer}
    class="file-tree-host flex-1 overflow-hidden"
    ondblclick={handleDblClick}
    oncontextmenu={handleContextMenu}
  ></div>
</div>
{/if}

{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={contextMenuItems()}
  />
{/if}
