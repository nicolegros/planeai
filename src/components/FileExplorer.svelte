<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";
  import { File, Folder, FolderOpen, ChevronRight, ChevronDown } from "@lucide/svelte";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { getSettings } from "../lib/settings.svelte";

  interface DirEntry {
    name: string;
    path: string;
    is_dir: boolean;
  }

  interface TreeNode {
    entry: DirEntry;
    children: TreeNode[] | null; // null = not loaded, [] = loaded empty
    expanded: boolean;
  }

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

  let { rootPath, sessionId, visible, activeFilePath = null, modifiedPaths = new Set(), onOpenFile, onPinFile, onFocus }: Props = $props();

  let panelWidth = $state(getLayoutWidth("file-explorer", 220));
  let tree = $state<TreeNode[]>([]);
  let flatList = $state<TreeNode[]>([]);
  let selectedIndex = $state(0);
  let contextMenu = $state<{ x: number; y: number; node: TreeNode } | null>(null);
  let renamingNode = $state<TreeNode | null>(null);
  let renameValue = $state("");
  let creatingIn = $state<{ parentPath: string; isDir: boolean } | null>(null);
  let createValue = $state("");
  let previewPath = $state<string | null>(null);
  let unlisten: (() => void) | null = null;

  function flatten(nodes: TreeNode[]): TreeNode[] {
    const result: TreeNode[] = [];
    for (const node of nodes) {
      result.push(node);
      if (node.expanded && node.children) {
        result.push(...flatten(node.children));
      }
    }
    return result;
  }

  function rebuildFlat() {
    flatList = flatten(tree);
  }

  async function loadChildren(path: string): Promise<TreeNode[]> {
    const entries: DirEntry[] = await invoke("fe_list_directory", { path });
    return entries.map(e => ({ entry: e, children: e.is_dir ? null : null, expanded: false }));
  }

  async function loadRoot() {
    tree = await loadChildren(rootPath);
    rebuildFlat();
  }

  async function toggleExpand(node: TreeNode) {
    if (!node.entry.is_dir) return;
    if (node.expanded) {
      node.expanded = false;
    } else {
      if (node.children === null) {
        node.children = await loadChildren(node.entry.path);
      }
      node.expanded = true;
    }
    rebuildFlat();
  }

  function relativePath(absolutePath: string): string {
    if (absolutePath.startsWith(rootPath)) {
      const rel = absolutePath.slice(rootPath.length);
      return rel.startsWith("/") ? rel.slice(1) : rel;
    }
    return absolutePath;
  }

  function openEntry(node: TreeNode) {
    if (node.entry.is_dir) {
      toggleExpand(node);
    } else {
      const rel = relativePath(node.entry.path);
      if (previewPath && previewPath !== rel) {
        // Replace preview buffer
      }
      previewPath = rel;
      onOpenFile(rel);
    }
  }

  function pinEntry(node: TreeNode) {
    if (!node.entry.is_dir) {
      const rel = relativePath(node.entry.path);
      previewPath = null;
      onPinFile(rel);
    }
  }

  function depth(node: TreeNode): number {
    const rel = relativePath(node.entry.path);
    return rel.split("/").length - 1;
  }

  // Keyboard navigation
  function handleKeydown(e: KeyboardEvent) {
    if (renamingNode || creatingIn) return;
    const vimMode = getSettings().vim_mode ?? true;

    const isDown = e.key === "ArrowDown" || (vimMode && e.key === "j");
    const isUp = e.key === "ArrowUp" || (vimMode && e.key === "k");
    const isExpand = e.key === "ArrowRight" || (vimMode && e.key === "l");
    const isCollapse = e.key === "ArrowLeft" || (vimMode && e.key === "h");
    const isOpen = e.key === "Enter";

    if (isDown) {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, flatList.length - 1);
    } else if (isUp) {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (isExpand && flatList[selectedIndex]?.entry.is_dir) {
      e.preventDefault();
      const node = flatList[selectedIndex];
      if (!node.expanded) toggleExpand(node);
    } else if (isCollapse && flatList[selectedIndex]?.entry.is_dir) {
      e.preventDefault();
      const node = flatList[selectedIndex];
      if (node.expanded) toggleExpand(node);
    } else if (isOpen && flatList[selectedIndex]) {
      e.preventDefault();
      openEntry(flatList[selectedIndex]);
    }
  }

  // Context menu actions
  function showContextMenu(e: MouseEvent, node: TreeNode) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, node };
  }

  function startRename(node: TreeNode) {
    renamingNode = node;
    renameValue = node.entry.name;
  }

  async function commitRename() {
    if (!renamingNode) return;
    const trimmed = renameValue.trim();
    if (trimmed && trimmed !== renamingNode.entry.name) {
      const parentPath = renamingNode.entry.path.replace(/\/[^/]+$/, "");
      const newPath = parentPath + "/" + trimmed;
      await invoke("fe_rename_entry", { oldPath: renamingNode.entry.path, newPath });
      renamingNode.entry.name = trimmed;
      renamingNode.entry.path = newPath;
    }
    renamingNode = null;
  }

  async function deleteEntry(node: TreeNode) {
    await invoke("fe_delete_to_trash", { path: node.entry.path });
    // Remove from parent's children
    removeNodeFromTree(tree, node);
    rebuildFlat();
  }

  function removeNodeFromTree(nodes: TreeNode[], target: TreeNode): boolean {
    const idx = nodes.indexOf(target);
    if (idx >= 0) { nodes.splice(idx, 1); return true; }
    for (const n of nodes) {
      if (n.children && removeNodeFromTree(n.children, target)) return true;
    }
    return false;
  }

  function startCreate(parentPath: string, isDir: boolean) {
    creatingIn = { parentPath, isDir };
    createValue = "";
  }

  async function commitCreate() {
    if (!creatingIn) return;
    const trimmed = createValue.trim();
    if (trimmed) {
      const fullPath = creatingIn.parentPath + "/" + trimmed;
      if (creatingIn.isDir) {
        await invoke("fe_create_directory", { path: fullPath });
      } else {
        await invoke("fe_create_file", { path: fullPath });
      }
      // Refresh the parent directory
      await refreshDir(creatingIn.parentPath);
    }
    creatingIn = null;
  }

  async function refreshDir(dirPath: string) {
    if (dirPath === rootPath) {
      await loadRoot();
      return;
    }
    // Find node and refresh children
    const node = findNodeByPath(tree, dirPath);
    if (node && node.expanded) {
      node.children = await loadChildren(dirPath);
      rebuildFlat();
    }
  }

  function findNodeByPath(nodes: TreeNode[], path: string): TreeNode | null {
    for (const n of nodes) {
      if (n.entry.path === path) return n;
      if (n.children) {
        const found = findNodeByPath(n.children, path);
        if (found) return found;
      }
    }
    return null;
  }

  // Filesystem watcher integration
  async function setupWatcher() {
    await invoke("fe_watch_directory", { sessionId, path: rootPath });
    const unlistenFn = await listen<{ session_id: string; path: string }>("fs-change", (event) => {
      if (event.payload.session_id !== sessionId) return;
      // Determine which directory was affected and refresh it
      const changedPath = event.payload.path;
      const parentDir = changedPath.replace(/\/[^/]+$/, "");
      refreshDir(parentDir);
    });
    unlisten = unlistenFn;
  }

  async function teardownWatcher() {
    if (unlisten) { unlisten(); unlisten = null; }
    await invoke("fe_unwatch_directory", { sessionId }).catch(() => {});
  }

  function autofocus(node: HTMLInputElement) {
    requestAnimationFrame(() => node.focus());
  }

  onMount(async () => {
    if (visible && rootPath) {
      await loadRoot();
      await setupWatcher();
    }
  });

  onDestroy(async () => {
    await teardownWatcher();
  });

  $effect(() => {
    if (visible && rootPath && tree.length === 0) {
      loadRoot();
      setupWatcher();
    }
  });

  function contextMenuItems(node: TreeNode) {
    const items = [];
    if (node.entry.is_dir) {
      items.push({ label: "New file", onSelect: () => startCreate(node.entry.path, false) });
      items.push({ label: "New folder", onSelect: () => startCreate(node.entry.path, true) });
    }
    items.push({ label: "Rename", onSelect: () => startRename(node) });
    items.push({ label: "Delete", danger: true, onSelect: () => deleteEntry(node) });
    return items;
  }
</script>

{#if visible}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<aside
  class="relative shrink-0 flex flex-col border-r border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 overflow-hidden"
  style:width="{panelWidth}px"
  onclick={onFocus}
  onkeydown={handleKeydown}
>
  <ResizeHandle side="right" bind:width={panelWidth} min={140} max={500} defaultWidth={220} onResizeEnd={(w) => setLayoutWidth("file-explorer", w)} />

  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-2 border-b border-surface-200 dark:border-surface-800">
    <span class="text-xs font-semibold text-surface-700 dark:text-surface-300 uppercase tracking-wider">Files</span>
  </div>

  <!-- Tree -->
  <nav class="flex-1 overflow-y-auto py-1 text-sm" role="tree">
    {#each flatList as node, i (node.entry.path)}
      {@const indent = depth(node)}
      {@const isSelected = i === selectedIndex}
      {@const isActive = activeFilePath && node.entry.path.endsWith(activeFilePath)}
      {@const isModified = modifiedPaths.has(relativePath(node.entry.path))}

      {#if renamingNode === node}
        <div class="flex items-center gap-1 px-2 py-0.5" style:padding-left="{indent * 12 + 8}px">
          <input
            use:autofocus
            class="flex-1 px-1 py-0.5 rounded text-xs bg-surface-100 dark:bg-surface-800 border border-primary-500 outline-none"
            bind:value={renameValue}
            onkeydown={(e) => { if (e.key === "Enter") commitRename(); if (e.key === "Escape") { renamingNode = null; } }}
            onblur={() => commitRename()}
          />
        </div>
      {:else}
        <button
          class="w-full text-left flex items-center gap-1 px-2 py-0.5 cursor-pointer transition-colors
            {isActive ? 'bg-primary-500/15 text-primary-700 dark:text-surface-50' : ''}
            {isSelected ? 'ring-1 ring-inset ring-primary-500/50' : ''}
            {!isActive && !isSelected ? 'text-surface-700 dark:text-surface-300 hover:bg-surface-100 dark:hover:bg-surface-800' : ''}"
          style:padding-left="{indent * 12 + 8}px"
          onclick={() => { selectedIndex = i; openEntry(node); }}
          ondblclick={() => pinEntry(node)}
          oncontextmenu={(e) => showContextMenu(e, node)}
          role="treeitem"
          aria-expanded={node.entry.is_dir ? node.expanded : undefined}
        >
          <!-- Expand chevron for dirs -->
          {#if node.entry.is_dir}
            {#if node.expanded}
              <ChevronDown class="size-3 shrink-0 text-surface-500" />
              <FolderOpen class="size-3.5 shrink-0 text-surface-500 dark:text-surface-400" />
            {:else}
              <ChevronRight class="size-3 shrink-0 text-surface-500" />
              <Folder class="size-3.5 shrink-0 text-surface-500 dark:text-surface-400" />
            {/if}
          {:else}
            <span class="size-3 shrink-0"></span>
            <File class="size-3.5 shrink-0 text-surface-500 dark:text-surface-400" />
          {/if}

          <span class="truncate {previewPath === relativePath(node.entry.path) ? 'italic' : ''}">{node.entry.name}</span>
          {#if isModified}<span class="ml-auto shrink-0 text-yellow-400">●</span>{/if}
        </button>
      {/if}
    {/each}

    {#if creatingIn}
      <div class="flex items-center gap-1 px-2 py-0.5" style:padding-left="20px">
        {#if creatingIn.isDir}
          <Folder class="size-3.5 shrink-0 text-surface-500" />
        {:else}
          <File class="size-3.5 shrink-0 text-surface-500" />
        {/if}
        <input
          use:autofocus
          class="flex-1 px-1 py-0.5 rounded text-xs bg-surface-100 dark:bg-surface-800 border border-primary-500 outline-none"
          bind:value={createValue}
          onkeydown={(e) => { if (e.key === "Enter") commitCreate(); if (e.key === "Escape") { creatingIn = null; } }}
          onblur={() => commitCreate()}
        />
      </div>
    {/if}
  </nav>
</aside>
{/if}

{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={contextMenuItems(contextMenu.node)}
  />
{/if}
