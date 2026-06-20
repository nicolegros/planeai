<script lang="ts">
  import { git } from "../lib/api";
  import type { ChangedFile, FileDiff as FileDiffData } from "../lib/types";
  import { onMount, onDestroy } from "svelte";
  import { FileDiff, type FileContents, type FileDiffOptions, type DiffLineAnnotation, type SelectedLineRange } from "@pierre/diffs";
  import { isDark } from "../lib/settings.svelte";
  import { getActiveZone } from "../lib/focus.svelte";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { ResizeHandle } from "./ui";
  import { addComment, removeComment, getComments, getFileCommentCount, getTotalCommentCount, type ReviewComment } from "../lib/review-comments.svelte";
  import { MessageSquare, X } from "@lucide/svelte";

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
  let renderer: FileDiff<ReviewComment> | null = null;
  let diffCache = new Map<string, FileDiffData>();

  // Comment input state
  let showCommentInput = $state(false);
  let commentText = $state("");
  let commentStartLine = $state(0);
  let commentEndLine = $state(0);
  let commentType = $state<"line" | "hunk" | "file">("line");
  let selectedRange = $state<SelectedLineRange | null>(null);
  let commentInputEl: HTMLTextAreaElement | undefined;

  // Reactive comment count for badge
  let totalCount = $derived(getTotalCommentCount(sessionId));

  function currentFilePath(): string {
    return files[selectedIndex]?.path ?? "";
  }

  function getAnnotationsForFile(filePath: string): DiffLineAnnotation<ReviewComment>[] {
    return getComments(sessionId)
      .filter((c) => c.filePath === filePath && c.type !== "file")
      .map((c) => ({ side: "additions" as const, lineNumber: c.startLine, metadata: c }));
  }

  function renderAnnotation(annotation: DiffLineAnnotation<ReviewComment>): HTMLElement | undefined {
    const comment = annotation.metadata;
    if (!comment) return undefined;

    const wrapper = document.createElement("div");
    wrapper.className = "review-comment-annotation";
    wrapper.style.cssText = "padding:6px 10px;margin:2px 0;border-radius:4px;font-size:12px;line-height:1.4;display:flex;align-items:flex-start;gap:8px;background:var(--comment-bg,rgba(128,128,128,0.1));border:1px solid var(--comment-border,rgba(128,128,128,0.2))";

    const textEl = document.createElement("span");
    textEl.style.cssText = "flex:1;white-space:pre-wrap;word-break:break-word";
    textEl.textContent = comment.text;

    const authorEl = document.createElement("span");
    authorEl.style.cssText = "font-size:10px;color:var(--comment-muted,#888);white-space:nowrap";
    authorEl.textContent = "You";

    const deleteBtn = document.createElement("button");
    deleteBtn.style.cssText = "background:none;border:none;cursor:pointer;padding:2px;color:var(--comment-muted,#888);font-size:14px;line-height:1";
    deleteBtn.textContent = "×";
    deleteBtn.title = "Delete comment";
    deleteBtn.onclick = () => {
      removeComment(sessionId, comment.id);
      rerenderDiff();
    };

    wrapper.appendChild(textEl);
    wrapper.appendChild(authorEl);
    wrapper.appendChild(deleteBtn);
    return wrapper;
  }

  function getThemeConfig(): FileDiffOptions<ReviewComment> {
    return {
      diffStyle,
      theme: { dark: "github-dark", light: "github-light" },
      themeType: isDark() ? "dark" : "light",
      disableFileHeader: true,
      preferredHighlighter: "shiki-js",
      tokenizeMaxLineLength: 1000,
      enableLineSelection: true,
      onLineSelected: (range) => { selectedRange = range; },
      renderAnnotation,
    };
  }

  function openCommentInput(start: number, end: number, type: "line" | "hunk" | "file") {
    commentStartLine = start;
    commentEndLine = end;
    commentType = type;
    commentText = "";
    showCommentInput = true;
    // Focus textarea after DOM update
    requestAnimationFrame(() => commentInputEl?.focus());
  }

  function submitComment() {
    const text = commentText.trim();
    if (!text) return;
    addComment(sessionId, {
      filePath: currentFilePath(),
      type: commentType,
      startLine: commentStartLine,
      endLine: commentEndLine,
      text,
    });
    cancelComment();
    rerenderDiff();
  }

  function cancelComment() {
    showCommentInput = false;
    commentText = "";
  }

  function handleCommentKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelComment();
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submitComment();
    }
  }

  function rerenderDiff() {
    if (!renderer || !diffContainer || files.length === 0) return;
    const file = files[selectedIndex];
    if (file) loadFileDiff(file);
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
    const annotations = getAnnotationsForFile(file.path);

    renderer.setOptions(getThemeConfig());
    renderer.render({ oldFile, newFile, fileContainer: diffContainer, lineAnnotations: annotations });
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
    showCommentInput = false;
    const file = files[index];
    if (file) {
      onFileChange?.(file.path.split("/").pop() || file.path);
      loadFileDiff(file);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!visible || getActiveZone() !== "terminal") return;
    if (showCommentInput) return; // let textarea handle keys

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
    } else if (e.key === "c" && !e.metaKey && !e.ctrlKey && files.length > 0) {
      e.preventDefault();
      if (selectedRange) {
        const type = selectedRange.start === selectedRange.end ? "line" : "hunk";
        openCommentInput(selectedRange.start, selectedRange.end, type);
      } else {
        openCommentInput(0, 0, "file");
      }
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
      renderer.setOptions({ ...getThemeConfig(), diffStyle: style, themeType: dark ? "dark" : "light" });
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
      <div class="flex-1"></div>
      {#if files.length > 0}
        <button
          class="text-xs px-2 py-0.5 rounded text-surface-500 hover:text-surface-700 dark:hover:text-surface-400 hover:bg-surface-200 dark:hover:bg-surface-700"
          onclick={() => openCommentInput(0, 0, "file")}
          title="Add file-level comment"
        >
          <MessageSquare size={12} />
        </button>
      {/if}
      {#if totalCount > 0}
        <span class="text-xs text-surface-500 dark:text-surface-400">{totalCount} comment{totalCount !== 1 ? 's' : ''}</span>
      {/if}
    </div>

    <!-- File-level comment input -->
    {#if showCommentInput && commentType === "file"}
      <div class="absolute top-8 left-0 right-0 z-20 p-2 border-b border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-800">
        <textarea
          bind:this={commentInputEl}
          bind:value={commentText}
          onkeydown={handleCommentKeydown}
          class="w-full p-2 text-xs rounded border border-surface-300 dark:border-surface-600 bg-white dark:bg-surface-900 text-surface-900 dark:text-surface-100 resize-none focus:outline-none focus:ring-1 focus:ring-primary-500"
          rows="3"
          placeholder="File-level comment… (Enter to submit, Shift+Enter for newline, Esc to cancel)"
        ></textarea>
      </div>
    {/if}

    <!-- Line/hunk comment input (shown below the toolbar area) -->
    {#if showCommentInput && commentType !== "file"}
      <div class="absolute top-8 left-0 right-0 z-20 p-2 border-b border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-800">
        <div class="text-[10px] text-surface-500 mb-1">
          Comment on {commentType === "hunk" ? `lines ${commentStartLine}–${commentEndLine}` : `line ${commentStartLine}`}
        </div>
        <textarea
          bind:this={commentInputEl}
          bind:value={commentText}
          onkeydown={handleCommentKeydown}
          class="w-full p-2 text-xs rounded border border-surface-300 dark:border-surface-600 bg-white dark:bg-surface-900 text-surface-900 dark:text-surface-100 resize-none focus:outline-none focus:ring-1 focus:ring-primary-500"
          rows="3"
          placeholder="Add comment… (Enter to submit, Shift+Enter for newline, Esc to cancel)"
        ></textarea>
      </div>
    {/if}

    <diffs-container bind:this={diffContainer} class="absolute inset-0 top-8 overflow-auto" style="display:block"></diffs-container>
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
        {@const fileCount = getFileCommentCount(sessionId, file.path)}
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
          {#if fileCount > 0}
            <span class="flex items-center gap-0.5 text-[10px] text-primary-600 dark:text-primary-400">
              <MessageSquare size={10} />{fileCount}
            </span>
          {/if}
          <span class="text-green-600 dark:text-green-300 text-[10px]">+{file.additions}</span>
          <span class="text-red-600 dark:text-red-300 text-[10px]">-{file.deletions}</span>
        </li>
      {/each}
    </ul>
  </div>
</div>
