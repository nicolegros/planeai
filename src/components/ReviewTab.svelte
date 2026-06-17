<script lang="ts">
  import { git } from "../lib/api";
  import type { ChangedFile, FileDiff as FileDiffData } from "../lib/types";
  import { onMount, onDestroy } from "svelte";
  import { FileDiff, type FileContents, type FileDiffOptions } from "@pierre/diffs";
  import { isDark } from "../lib/settings.svelte";
  import { getActiveZone } from "../lib/focus.svelte";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { ResizeHandle } from "./ui";

  interface Props {
    repoPath: string;
    baseBranch: string;
    visible: boolean;
    sessionId: string;
    onEditFile?: (filePath: string) => void;
    onFileChange?: (fileName: string) => void;
  }

  let { repoPath, baseBranch, visible, sessionId, onEditFile, onFileChange }: Props = $props();

  let files = $state<ChangedFile[]>([]);
  let selectedIndex = $state(0);
  let loading = $state(true);
  let diffStyle = $state<"split" | "unified">("split");
  let sidebarWidth = $state(getLayoutWidth("diff-sidebar", 256));

  let diffContainer: HTMLElement;
  let renderer: FileDiff | null = null;
  let diffCache = new Map<string, FileDiffData>();

  function getThemeConfig(): FileDiffOptions<undefined> {
    return {
      diffStyle,
      theme: { dark: "github-dark", light: "github-light" },
      themeType: isDark() ? "dark" : "light",
      disableFileHeader: true,
    };
  }

  async function refresh() {
    loading = true;
    diffCache.clear();
    try {
      files = await git.getChangedFiles(repoPath, baseBranch);
      if (files.length > 0 && selectedIndex >= files.length) selectedIndex = 0;
      if (files.length > 0) {
        await loadFileDiff(files[selectedIndex]);
        onFileChange?.(files[selectedIndex].path.split("/").pop() || files[selectedIndex].path);
      }
    } catch (e) {
      console.error("Failed to get changed files:", e);
      files = [];
    }
    loading = false;
  }

  async function loadFileDiff(file: ChangedFile) {
    if (!renderer || !diffContainer) return;
    const cached = diffCache.get(file.path);
    const diff = cached ?? await fetchDiff(file);
    if (!diff) return;

    const oldFile: FileContents = { name: file.old_path || file.path, contents: diff.original };
    const newFile: FileContents = { name: file.path, contents: diff.modified };

    renderer.setOptions(getThemeConfig());
    renderer.render({ oldFile, newFile, fileContainer: diffContainer });
  }

  async function fetchDiff(file: ChangedFile): Promise<FileDiffData | null> {
    try {
      const diff = await git.getFileDiff(repoPath, baseBranch, file.path, file.old_path);
      diffCache.set(file.path, diff);
      return diff;
    } catch (e) {
      console.error("Failed to get file diff:", e);
      return null;
    }
  }

  function selectFile(index: number) {
    selectedIndex = index;
    const file = files[index];
    if (file) {
      onFileChange?.(file.path.split("/").pop() || file.path);
      loadFileDiff(file);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!visible || getActiveZone() !== "terminal") return;

    if (e.key === "ArrowDown" || (e.key === "j" && !e.metaKey && !e.ctrlKey)) {
      e.preventDefault();
      if (selectedIndex < files.length - 1) selectFile(selectedIndex + 1);
    } else if (e.key === "ArrowUp" || (e.key === "k" && !e.metaKey && !e.ctrlKey)) {
      e.preventDefault();
      if (selectedIndex > 0) selectFile(selectedIndex - 1);
    } else if (e.key === "n" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      scrollToHunk("next");
    } else if (e.key === "p" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      scrollToHunk("prev");
    } else if (e.key === "u" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      diffStyle = diffStyle === "split" ? "unified" : "split";
    } else if (e.key === "r" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      refresh();
    } else if (e.key === "e" && !e.metaKey && !e.ctrlKey && files.length > 0) {
      e.preventDefault();
      onEditFile?.(files[selectedIndex].path);
    }
  }

  function scrollToHunk(direction: "next" | "prev") {
    if (!diffContainer) return;
    const separators = diffContainer.querySelectorAll("[data-hunk-separator]");
    if (separators.length === 0) return;

    const containerTop = diffContainer.scrollTop;
    const items = Array.from(separators);

    if (direction === "next") {
      const next = items.find((el) => (el as HTMLElement).offsetTop > containerTop + 10);
      if (next) next.scrollIntoView({ block: "start", behavior: "smooth" });
    } else {
      const prev = items.reverse().find((el) => (el as HTMLElement).offsetTop < containerTop - 10);
      if (prev) prev.scrollIntoView({ block: "start", behavior: "smooth" });
    }
  }

  let mounted = false;

  onMount(() => {
    window.addEventListener("keydown", handleKeydown);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeydown);
    renderer?.cleanUp();
    renderer = null;
  });

  $effect(() => {
    if (visible && !mounted && diffContainer) {
      mounted = true;
      renderer = new FileDiff(getThemeConfig());
      refresh();
    }
  });

  // Re-render when theme or diff style changes
  $effect(() => {
    const dark = isDark();
    const style = diffStyle;
    if (renderer && mounted && files.length > 0) {
      renderer.setOptions({ diffStyle: style, themeType: dark ? "dark" : "light" });
      renderer.setThemeType(dark ? "dark" : "light");
      loadFileDiff(files[selectedIndex]);
    }
  });

  function statusColor(status: string): string {
    switch (status) {
      case "A": return "text-green-600 dark:text-green-300";
      case "D": return "text-red-600 dark:text-red-300";
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
  <div class="flex-1 min-w-0 relative overflow-hidden">
    <div class="absolute top-0 left-0 right-0 h-8 flex items-center px-3 gap-2 border-b border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 z-10">
      <button
        class="text-xs px-2 py-0.5 rounded {diffStyle === 'split' ? 'bg-primary-100 dark:bg-primary-900 text-primary-700 dark:text-primary-300' : 'text-surface-500 hover:text-surface-700 dark:hover:text-surface-400'}"
        onclick={() => (diffStyle = "split")}
      >Split</button>
      <button
        class="text-xs px-2 py-0.5 rounded {diffStyle === 'unified' ? 'bg-primary-100 dark:bg-primary-900 text-primary-700 dark:text-primary-300' : 'text-surface-500 hover:text-surface-700 dark:hover:text-surface-400'}"
        onclick={() => (diffStyle = "unified")}
      >Unified</button>
    </div>
    <div bind:this={diffContainer} class="absolute inset-0 top-8 overflow-auto"></div>
    {#if loading && files.length === 0}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">Loading diff…</div>
    {:else if files.length === 0 && !loading}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">No changes on this branch</div>
    {/if}
  </div>

  <div class="relative shrink-0 border-l border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 overflow-y-auto" style:width="{sidebarWidth}px">
    <ResizeHandle side="left" bind:width={sidebarWidth} min={180} max={Infinity} defaultWidth={256} onResizeEnd={(w) => setLayoutWidth("diff-sidebar", w)} />
    <div class="px-3 py-2 text-xs font-medium text-surface-500 dark:text-surface-400 uppercase tracking-wider border-b border-surface-200 dark:border-surface-800">
      Changed files ({files.length})
    </div>
    <ul class="py-1" role="listbox">
      {#each files as file, i (file.path)}
        <li
          role="option"
          aria-selected={i === selectedIndex}
          class="px-2 py-1 cursor-pointer flex items-center gap-1 text-xs text-surface-700 dark:text-surface-200 hover:bg-surface-100 dark:hover:bg-surface-800 {i === selectedIndex ? 'bg-surface-200 dark:bg-surface-700' : ''}"
          onclick={() => selectFile(i)}
        >
          <span class="font-mono w-4 shrink-0 {statusColor(file.status)}">{file.status}</span>
          <span class="truncate flex-1" title={file.path}>
            <span class="text-surface-400">{dirName(file.path)}</span>{fileName(file.path)}
          </span>
          <span class="text-green-600 dark:text-green-300 text-[10px]">+{file.additions}</span>
          <span class="text-red-600 dark:text-red-300 text-[10px]">-{file.deletions}</span>
        </li>
      {/each}
    </ul>
  </div>
</div>
