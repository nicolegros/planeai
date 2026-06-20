<script lang="ts">
  import { git } from "../lib/api";
  import type { ChangedFile, FileDiff as FileDiffData } from "../lib/types";
  import { onMount, onDestroy, untrack } from "svelte";
  import { FileDiff, type FileContents, type FileDiffOptions, type DiffLineAnnotation, type SelectedLineRange, processFile, type FileDiffMetadata } from "@pierre/diffs";
  import { isDark } from "../lib/settings.svelte";
  import { getActiveZone } from "../lib/focus.svelte";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { ResizeHandle } from "./ui";
  import { addComment, removeComment, getComments, getFileCommentCount, getTotalCommentCount, clearComments, type ReviewComment } from "../lib/review-comments.svelte";
  import { MessageSquare, X, Send } from "@lucide/svelte";
  import { pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { serializeComments } from "../lib/review-serializer";
  import { getActiveSession } from "../lib/session-orchestrator.svelte";
  import Button from "./ui/Button.svelte";
  import { Dialog } from "bits-ui";
  import { warmHighlighter, prefetchLanguage, queueHighlightWork, buildCacheKey, getCachedHTML, setCachedHTML, invalidateCache } from "../lib/diff-highlight";

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
  let diffCache = new Map<string, string>();
  let parsedDiffCache = new Map<string, FileDiffMetadata>();
  let cachedLineNumbers: number[] | null = null;

  // Comment input state
  let showCommentInput = $state(false);
  let showHelp = $state(false);
  let diffFocus = $state<"list" | "body">("list");
  let commentText = $state("");
  let commentStartLine = $state(0);
  let commentEndLine = $state(0);
  let commentType = $state<"line" | "hunk" | "file">("line");
  let selectedRange = $state<SelectedLineRange | null>(null);
  let commentInputEl: HTMLTextAreaElement | undefined;
  let cursorLine = $state(1);
  let selectionAnchor = $state<number | null>(null);

  // Reactive comment count for badge
  let totalCount = $derived(getTotalCommentCount(sessionId));
  let sessionExited = $derived(getActiveSession()?.status === "exited");

  async function sendFeedback() {
    const comments = getComments(sessionId);
    if (comments.length === 0 || sessionExited) return;
    // Fetch file contents on-demand for serialization (not on every view)
    const filePaths = [...new Set(comments.map((c) => c.filePath))];
    const fileDiffs = new Map<string, FileDiffData>();
    await Promise.all(filePaths.map(async (path) => {
      try {
        const diff = await git.getFileDiff(repoPath, baseBranch, path, null);
        fileDiffs.set(path, diff);
      } catch { /* non-critical */ }
    }));
    const serialized = serializeComments(comments, fileDiffs);
    const bytes = Array.from(new TextEncoder().encode(serialized));
    await pty.write(sessionId, bytes);
    await pty.write(sessionId, [0x0d]);
    const count = comments.length;
    clearComments(sessionId);
    rerenderDiff();
    showSnackbar(`Feedback sent (${count} comment${count !== 1 ? "s" : ""})`, "success");
  }

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

  function handleLineSelected(range: SelectedLineRange | null) {
    if (range) {
      selectedRange = range;
      diffFocus = "body";
      cursorLine = range.start;
    }
  }

  function getThemeConfig(): FileDiffOptions<ReviewComment> {
    return {
      diffStyle,
      theme: { dark: "github-dark", light: "github-light" },
      themeType: isDark() ? "dark" : "light",
      disableFileHeader: true,
      preferredHighlighter: "shiki-wasm",
      tokenizeMaxLineLength: 1000,
      tokenizeMaxLength: 5000,
      enableLineSelection: true,
      onLineSelected: handleLineSelected,
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
    clearSelection();
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
    parsedDiffCache.clear();
    invalidateCache();
    try {
      files = await git.getChangedFiles(repoPath, baseBranch);
      if (files.length > 0 && selectedIndex >= files.length) selectedIndex = 0;
      if (files.length > 0) {
        // Prefetch all patches in parallel (IPC is fast, git diff is native)
        const prefetchPromises = files.map((f) => fetchPatch(f));
        // Load the selected file immediately (don't wait for all)
        await loadFileDiff(files[selectedIndex]);
        onFileChange?.(files[selectedIndex].path.split("/").pop() || files[selectedIndex].path);
        // Let the rest finish in background
        Promise.all(prefetchPromises).catch(() => {});
      }
    } catch (e) {
      console.error("Failed to get changed files:", e);
      files = [];
    }
    loading = false;
  }

  function prefetchAdjacentFiles(index: number) {
    const theme = { dark: "github-dark", light: "github-light" };
    const neighbors = [index - 1, index + 1, index + 2];
    for (const i of neighbors) {
      if (i >= 0 && i < files.length) {
        prefetchLanguage(files[i].path, theme);
        if (!diffCache.has(files[i].path)) {
          fetchPatch(files[i]);
        }
      }
    }
  }

  async function loadFileDiff(file: ChangedFile) {
    if (!renderer || !diffContainer) return;
    cachedLineNumbers = null;

    let fileDiff = parsedDiffCache.get(file.path);
    if (!fileDiff) {
      const patch = diffCache.get(file.path) ?? await fetchPatch(file);
      if (!patch) return;
      fileDiff = processFile(patch) as FileDiffMetadata | undefined;
      if (!fileDiff) return;
      parsedDiffCache.set(file.path, fileDiff);
    }

    const annotations = getAnnotationsForFile(file.path);
    renderer.render({ fileDiff, fileContainer: diffContainer, lineAnnotations: annotations });
    cachedLineNumbers = null;
  }

  async function fetchPatch(file: ChangedFile): Promise<string | null> {
    try {
      const patch = await git.getFilePatch(repoPath, baseBranch, file.path, file.old_path ?? null);
      if (patch) diffCache.set(file.path, patch);
      return patch || null;
    } catch (e) {
      console.error("Failed to get file patch:", e);
      return null;
    }
  }

  function selectFile(index: number) {
    selectedIndex = index;
    showCommentInput = false;
    cursorLine = 1;
    selectionAnchor = null;
    selectedRange = null;
    renderer?.setSelectedLines(null);
    const file = files[index];
    if (file) {
      onFileChange?.(file.path.split("/").pop() || file.path);
      loadFileDiff(file);
      prefetchAdjacentFiles(index);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!visible || getActiveZone() !== "terminal") return;

    if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      sendFeedback();
      return;
    }

    if (showCommentInput) return; // let textarea handle keys

    // ─── Global keys (both modes) ─────────────────────────────────────
    if (e.key === "?" || (e.key === "/" && e.shiftKey)) {
      e.preventDefault();
      showHelp = !showHelp;
      return;
    }
    if ((e.key === "n" && e.ctrlKey && !e.metaKey) || (e.key === "ArrowDown" && e.ctrlKey)) {
      e.preventDefault();
      if (selectedIndex < files.length - 1) navigateFile(selectedIndex + 1);
      return;
    }
    if ((e.key === "p" && e.ctrlKey && !e.metaKey) || (e.key === "ArrowUp" && e.ctrlKey)) {
      e.preventDefault();
      if (selectedIndex > 0) navigateFile(selectedIndex - 1);
      return;
    }
    if (e.key === "]" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      scrollToHunk("next");
      return;
    }
    if (e.key === "[" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      scrollToHunk("prev");
      return;
    }
    if (e.key === "u" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      diffStyle = diffStyle === "split" ? "unified" : "split";
      return;
    }
    if (e.key === "r" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      refresh();
      cursorLine = 1;
      return;
    }
    if (e.key === "e" && !e.metaKey && !e.ctrlKey && files.length > 0) {
      e.preventDefault();
      onEditFile?.(files[selectedIndex].path);
      return;
    }

    // ─── List mode ────────────────────────────────────────────────────
    if (diffFocus === "list") {
      if (e.key === "ArrowDown" || (e.key === "j" && !e.metaKey && !e.ctrlKey)) {
        e.preventDefault();
        if (selectedIndex < files.length - 1) selectFile(selectedIndex + 1);
      } else if (e.key === "ArrowUp" || (e.key === "k" && !e.metaKey && !e.ctrlKey)) {
        e.preventDefault();
        if (selectedIndex > 0) selectFile(selectedIndex - 1);
      } else if (e.key === "Enter" && !e.metaKey) {
        e.preventDefault();
        diffFocus = "body";
        cursorLine = getFirstVisibleLine();
        showCursor();
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (showHelp) showHelp = false;
      }
      return;
    }

    // ─── Body mode ────────────────────────────────────────────────────
    if (e.key === "Escape") {
      e.preventDefault();
      if (showHelp) { showHelp = false; return; }
      if (selectionAnchor !== null) { clearSelection(); return; }
      diffFocus = "list";
      clearSelection();
      return;
    }
    if (e.key === "ArrowDown" || (e.key === "j" && !e.metaKey && !e.ctrlKey && !e.shiftKey)) {
      e.preventDefault();
      moveCursorLine(1);
    } else if (e.key === "ArrowUp" || (e.key === "k" && !e.metaKey && !e.ctrlKey && !e.shiftKey)) {
      e.preventDefault();
      moveCursorLine(-1);
    } else if (e.key === "ArrowDown" && e.shiftKey) {
      e.preventDefault();
      if (selectionAnchor === null) selectionAnchor = cursorLine;
      moveCursorLine(1);
    } else if (e.key === "ArrowUp" && e.shiftKey) {
      e.preventDefault();
      if (selectionAnchor === null) selectionAnchor = cursorLine;
      moveCursorLine(-1);
    } else if (e.key === "v" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      toggleSelectionMode();
    } else if (e.key === "d" && !e.metaKey && !e.ctrlKey || e.key === "f" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      moveCursorLine(Math.floor((diffContainer?.clientHeight ?? 400) / 40));
    } else if (e.key === "b" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      moveCursorLine(-Math.floor((diffContainer?.clientHeight ?? 400) / 40));
    } else if (e.key === "g" && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      cursorLine = 1;
      moveCursorLine(0);
    } else if (e.key === "G" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      cursorLine = 99999;
      moveCursorLine(0);
    } else if (e.key === "c" && !e.metaKey && !e.ctrlKey && files.length > 0) {
      e.preventDefault();
      if (selectedRange) {
        const type = selectedRange.start === selectedRange.end ? "line" : "hunk";
        openCommentInput(selectedRange.start, selectedRange.end, type);
      } else {
        openCommentInput(cursorLine, cursorLine, "line");
      }
    }
  }

  function navigateFile(index: number) {
    selectFile(index);
    if (diffFocus === "body") {
      cursorLine = 1;
      requestAnimationFrame(() => showCursor());
    }
  }

  function showCursor() {
    renderer?.setSelectedLines({ start: cursorLine, end: cursorLine, side: "additions" }, { scroll: true });
    selectedRange = { start: cursorLine, end: cursorLine, side: "additions" };
  }

  function getVisibleLineNumbers(): number[] {
    if (cachedLineNumbers) return cachedLineNumbers;
    if (!diffContainer) return [];
    const items = diffContainer.querySelectorAll("[data-additions] [data-column-number]");
    const seen = new Set<number>();
    const lines: number[] = [];
    for (const el of items) {
      const n = parseInt((el as HTMLElement).dataset.columnNumber ?? "", 10);
      if (!isNaN(n) && n > 0 && !seen.has(n)) { seen.add(n); lines.push(n); }
    }
    // Fall back to any gutter column if additions side isn't found (unified mode)
    if (lines.length === 0) {
      const all = diffContainer.querySelectorAll("[data-column-number]");
      for (const el of all) {
        const n = parseInt((el as HTMLElement).dataset.columnNumber ?? "", 10);
        if (!isNaN(n) && n > 0 && !seen.has(n)) { seen.add(n); lines.push(n); }
      }
    }
    cachedLineNumbers = lines.sort((a, b) => a - b);
    return cachedLineNumbers;
  }

  function moveCursorLine(delta: number) {
    const lines = getVisibleLineNumbers();
    if (lines.length === 0) return;

    if (delta === 0) {
      // Just clamp to nearest available line
      cursorLine = lines.reduce((prev, curr) =>
        Math.abs(curr - cursorLine) < Math.abs(prev - cursorLine) ? curr : prev
      );
    } else {
      const currentIdx = lines.findIndex((l) => l >= cursorLine);
      const idx = currentIdx === -1 ? lines.length - 1 : currentIdx;
      const nextIdx = Math.max(0, Math.min(lines.length - 1, idx + delta));
      cursorLine = lines[nextIdx];
    }

    if (selectionAnchor !== null) {
      const start = Math.min(selectionAnchor, cursorLine);
      const end = Math.max(selectionAnchor, cursorLine);
      selectedRange = { start, end, side: "additions" };
      renderer?.setSelectedLines({ start, end, side: "additions" }, { scroll: true });
    } else {
      selectedRange = { start: cursorLine, end: cursorLine, side: "additions" };
      renderer?.setSelectedLines({ start: cursorLine, end: cursorLine, side: "additions" }, { scroll: true });
    }
  }

  function toggleSelectionMode() {
    if (selectionAnchor !== null) {
      // Already in selection mode — exit but keep the range
      selectionAnchor = null;
    } else {
      // Enter selection mode at cursor
      selectionAnchor = cursorLine;
      selectedRange = { start: cursorLine, end: cursorLine, side: "additions" };
      renderer?.setSelectedLines({ start: cursorLine, end: cursorLine, side: "additions" });
    }
  }

  function clearSelection() {
    selectionAnchor = null;
    selectedRange = null;
    renderer?.setSelectedLines(null);
  }

  function scrollToHunk(direction: "next" | "prev") {
    if (!diffContainer) return;
    const separators = diffContainer.querySelectorAll("[data-separator]");
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

  function getFirstVisibleLine(): number {
    if (!diffContainer) return 1;
    const scrollTop = diffContainer.scrollTop;
    const items = diffContainer.querySelectorAll("[data-column-number]");
    for (const el of items) {
      if ((el as HTMLElement).offsetTop >= scrollTop) {
        const num = parseInt((el as HTMLElement).dataset.columnNumber ?? "", 10);
        if (!isNaN(num) && num > 0) return num;
      }
    }
    const lines = getVisibleLineNumbers();
    return lines[0] ?? 1;
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
      // Warm highlighter in background — subsequent renders will have color
      warmHighlighter({ dark: "github-dark", light: "github-light" });
    }
  });

  // Re-render when theme or diff style changes
  $effect(() => {
    const dark = isDark();
    const style = diffStyle;
    if (renderer && mounted) {
      cachedLineNumbers = null;
      renderer.setOptions({ ...getThemeConfig(), diffStyle: style, themeType: dark ? "dark" : "light" });
      renderer.setThemeType(dark ? "dark" : "light");
      untrack(() => rerenderDiff());
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
        <Button variant="primary" size="sm" onclick={sendFeedback} disabled={sessionExited} title={sessionExited ? "Agent is not running" : "Send feedback to agent (⌘Enter)"}>
          <Send size={12} />
          <span class="ml-1">Send Feedback ({totalCount})</span>
        </Button>
      {/if}
      <button
        class="text-xs px-1.5 py-0.5 rounded text-surface-400 hover:text-surface-600 dark:hover:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-700"
        onclick={() => (showHelp = !showHelp)}
        title="Keyboard shortcuts (?)"
      >?</button>
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

    <diffs-container bind:this={diffContainer} class="absolute inset-0 top-8 overflow-auto {diffFocus === 'body' ? 'ring-1 ring-primary-400/40 ring-inset' : ''}" style="display:block"></diffs-container>
    {#if showHelp}
      <Dialog.Root open={showHelp} onOpenChange={(v) => (showHelp = v)}>
        <Dialog.Portal>
          <Dialog.Content
            class="fixed left-1/2 top-1/2 z-50 w-full max-w-xs -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-200 bg-surface-50 p-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none"
          >
            <Dialog.Title class="text-sm font-medium text-surface-900 dark:text-surface-50 mb-3">Review Shortcuts</Dialog.Title>
            <div class="text-xs text-surface-500 dark:text-surface-400 uppercase tracking-wide mb-1.5">List mode</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs text-surface-700 dark:text-surface-300 mb-3">
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">j/k ↓/↑</kbd><span>Navigate files</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Enter</kbd><span>Focus diff body</span>
            </div>
            <div class="text-xs text-surface-500 dark:text-surface-400 uppercase tracking-wide mb-1.5">Body mode</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs text-surface-700 dark:text-surface-300 mb-3">
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">j/k ↓/↑</kbd><span>Move cursor</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Shift+↓/↑</kbd><span>Extend selection</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">v</kbd><span>Toggle visual select</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">d/f  b</kbd><span>Half-page ↓ / ↑</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">g / G</kbd><span>Top / bottom</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">c</kbd><span>Comment on selection</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Esc</kbd><span>Back to list</span>
            </div>
            <div class="text-xs text-surface-500 dark:text-surface-400 uppercase tracking-wide mb-1.5">Both modes</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs text-surface-700 dark:text-surface-300">
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Ctrl+n/p</kbd><span>Next / prev file</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">] / [</kbd><span>Next / prev hunk</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">u</kbd><span>Split / unified</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">e</kbd><span>Edit file</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">r</kbd><span>Refresh</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">⌘↵</kbd><span>Send feedback</span>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}
    {#if loading && files.length === 0}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">Loading diff…</div>
    {:else if files.length === 0 && !loading}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">No changes on this branch</div>
    {/if}
  </div>

  <div class="relative shrink-0 border-l border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 overflow-y-auto transition-opacity {diffFocus === 'body' ? 'opacity-60' : ''}" style:width="{sidebarWidth}px">
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
