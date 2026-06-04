<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, onDestroy } from "svelte";
  import { CmDiffRenderer } from "../lib/cm-diff-renderer";
  import type { DiffRenderer } from "../lib/diff-renderer";
  import { getSettings } from "../lib/settings.svelte";

  interface ChangedFile {
    path: string;
    status: string;
    additions: number;
    deletions: number;
  }

  interface FileDiff {
    original: string;
    modified: string;
    language: string;
  }

  interface Props {
    repoPath: string;
    baseBranch: string;
    visible: boolean;
    theme?: string;
  }

  let { repoPath, baseBranch, visible, theme = "vs-dark" }: Props = $props();

  let files = $state<ChangedFile[]>([]);
  let selectedIndex = $state(0);
  let loading = $state(true);
  let diffMode = $state<'side-by-side' | 'unified'>('side-by-side');

  let renderer: DiffRenderer | null = null;
  let editorContainer: HTMLElement;

  async function refresh() {
    loading = true;
    try {
      files = await invoke<ChangedFile[]>("get_changed_files", { repoPath, baseBranch });
      if (files.length > 0 && selectedIndex >= files.length) {
        selectedIndex = 0;
      }
      if (files.length > 0) {
        await loadFileDiff(files[selectedIndex]);
      }
    } catch (e) {
      console.error("Failed to get changed files:", e);
      files = [];
    }
    loading = false;
  }

  async function loadFileDiff(file: ChangedFile) {
    if (!renderer) return;
    try {
      const diff = await invoke<FileDiff>("get_file_diff", { repoPath, baseBranch, filePath: file.path });
      renderer.setDiff(diff.original, diff.modified, diff.language);
    } catch (e) {
      console.error("Failed to get file diff:", e);
    }
  }

  function selectFile(index: number) {
    selectedIndex = index;
    loadFileDiff(files[index]);
  }

  function nextFile() {
    if (selectedIndex < files.length - 1) selectFile(selectedIndex + 1);
  }

  function prevFile() {
    if (selectedIndex > 0) selectFile(selectedIndex - 1);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!visible) return;

    // File navigation: ↑/↓, j/k, Tab/Shift+Tab, Ctrl+n/Ctrl+p
    if (e.key === "ArrowDown" || (e.key === "j" && !e.metaKey && !e.ctrlKey)) {
      e.preventDefault();
      nextFile();
    } else if (e.key === "ArrowUp" || (e.key === "k" && !e.metaKey && !e.ctrlKey)) {
      e.preventDefault();
      prevFile();
    } else if (e.key === "Tab" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      e.shiftKey ? prevFile() : nextFile();
    } else if (e.key === "n" && e.ctrlKey) {
      e.preventDefault();
      nextFile();
    } else if (e.key === "p" && e.ctrlKey) {
      e.preventDefault();
      prevFile();
    // Scroll: Ctrl+d half-page down, Ctrl+u half-page up
    } else if (e.key === "d" && e.ctrlKey) {
      e.preventDefault();
      const mv = (renderer as any)?.mergeView;
      if (mv) {
        const el = mv.b.scrollDOM;
        el.scrollTop += el.clientHeight / 2;
      }
    } else if (e.key === "u" && e.ctrlKey) {
      e.preventDefault();
      const mv = (renderer as any)?.mergeView;
      if (mv) {
        const el = mv.b.scrollDOM;
        el.scrollTop -= el.clientHeight / 2;
      }
    // Other
    } else if (e.key === "r" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      refresh();
    } else if (e.key === "u" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      diffMode = diffMode === 'side-by-side' ? 'unified' : 'side-by-side';
      renderer?.setMode(diffMode);
    }
  }

  let mounted = false;

  onMount(() => {
    window.addEventListener("keydown", handleKeydown);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeydown);
    renderer?.destroy();
    renderer = null;
  });

  $effect(() => {
    if (visible && !mounted && editorContainer) {
      mounted = true;
      renderer = new CmDiffRenderer();
      renderer.mount(editorContainer);
      renderer.setTheme(theme);
      refresh();
    } else if (visible && mounted) {
      refresh();
    }
  });

  $effect(() => {
    renderer?.setTheme(theme);
  });

  $effect(() => {
    const { font_family, font_size } = getSettings().terminal;
    renderer?.setFont(font_family, font_size);
  });

  function statusColor(status: string): string {
    switch (status) {
      case "A": return "text-green-500";
      case "D": return "text-red-500";
      case "M": return "text-yellow-500";
      case "R": return "text-blue-500";
      default: return "text-surface-400";
    }
  }

  function fileName(path: string): string {
    return path.split("/").pop() || path;
  }

  function dirName(path: string): string {
    const parts = path.split("/");
    return parts.length > 1 ? parts.slice(0, -1).join("/") + "/" : "";
  }
</script>

<div class="flex h-full w-full" class:hidden={!visible}>
  <!-- Diff content area -->
  <div class="flex-1 min-w-0 relative">
    <div bind:this={editorContainer} class="h-full w-full overflow-hidden"></div>
    {#if loading && files.length === 0}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">Loading diff…</div>
    {:else if files.length === 0}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">No changes on this branch</div>
    {/if}
  </div>

  <!-- Right sidebar file list -->
  <div class="w-64 shrink-0 border-l border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 overflow-y-auto">
    <div class="px-3 py-2 text-xs font-medium text-surface-500 dark:text-surface-400 uppercase tracking-wider border-b border-surface-200 dark:border-surface-800">
      Changed files ({files.length})
    </div>
    <ul class="py-1" role="listbox">
      {#each files as file, i (file.path)}
        <li
          role="option"
          aria-selected={i === selectedIndex}
          class="px-2 py-1 cursor-pointer flex items-center gap-1 text-xs hover:bg-surface-100 dark:hover:bg-surface-800 {i === selectedIndex ? 'bg-surface-200 dark:bg-surface-700' : ''}"
          onclick={() => selectFile(i)}
        >
          <span class="font-mono w-4 shrink-0 {statusColor(file.status)}">{file.status}</span>
          <span class="truncate flex-1" title={file.path}>
            <span class="text-surface-400">{dirName(file.path)}</span>{fileName(file.path)}
          </span>
          <span class="text-green-600 text-[10px]">+{file.additions}</span>
          <span class="text-red-600 text-[10px]">-{file.deletions}</span>
        </li>
      {/each}
    </ul>
  </div>
</div>
