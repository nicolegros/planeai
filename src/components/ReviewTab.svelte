<script lang="ts">
  import { git } from "../lib/api";
  import type { ChangedFile, FileDiff as FileDiffData } from "../lib/types";
  import { onMount, onDestroy } from "svelte";
  import { CodeView, parsePatchFiles, type CodeViewItem, type DiffLineAnnotation, type SelectedLineRange, type FileDiffMetadata } from "@pierre/diffs";
  import { getOrCreateWorkerPoolSingleton, terminateWorkerPoolSingleton } from "@pierre/diffs/worker";
  import { workerFactory } from "../lib/worker-factory";
  import { isDark } from "../lib/settings.svelte";
  import { getActiveZone } from "../lib/focus.svelte";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { ResizeHandle } from "./ui";
  import { addComment, removeComment, getComments, getFileCommentCount, getTotalCommentCount, clearComments, type ReviewComment } from "../lib/review-comments.svelte";
  import { MessageSquare, Send } from "@lucide/svelte";
  import { pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { serializeComments } from "../lib/review-serializer";
  import { getActiveSession } from "../lib/session-orchestrator.svelte";
  import Button from "./ui/Button.svelte";
  import { Dialog } from "bits-ui";
  import { getPreloadedPatches, clearPreloadedPatches } from "../lib/diff-preload";

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

  // Focus state
  let showCommentInput = $state(false);
  let showHelp = $state(false);
  let diffFocus = $state<"list" | "body">("list");
  let commentText = $state("");
  let commentStartLine = $state(0);
  let commentEndLine = $state(0);
  let commentType = $state<"line" | "hunk" | "file">("line");
  let cursorLine = $state(1);
  let selectionAnchor = $state<number | null>(null);
  let commentInputEl: HTMLTextAreaElement | undefined;

  // CodeView + Worker Pool
  let viewerRoot: HTMLElement;
  let viewer: CodeView | null = null;
  let mounted = false;

  let workerPool: ReturnType<typeof getOrCreateWorkerPoolSingleton> | null = null;

  function getWorkerPool() {
    if (!workerPool) {
      workerPool = getOrCreateWorkerPoolSingleton({
        poolOptions: { workerFactory },
        highlighterOptions: {
          theme: { dark: "github-dark", light: "github-light" },
          langs: ["typescript", "javascript", "css", "html", "rust", "python", "go", "svelte", "json", "yaml", "toml", "bash", "sql"],
        },
      });
    }
    return workerPool;
  }

  // Reactive
  let totalCount = $derived(getTotalCommentCount(sessionId));
  let sessionExited = $derived(getActiveSession()?.status === "exited");

  // ─── Core Functions ─────────────────────────────────────────────────────────

  function currentFileId(): string {
    return files[selectedIndex] ? `diff:${files[selectedIndex].path}` : "";
  }

  async function refresh() {
    loading = true;
    try {
      files = await git.getChangedFiles(repoPath, baseBranch);
      if (files.length > 0 && selectedIndex >= files.length) selectedIndex = 0;
      if (files.length > 0) {
        await loadAllDiffs();
        onFileChange?.(files[selectedIndex].path.split("/").pop() || files[selectedIndex].path);
      }
    } catch (e) {
      console.error("Failed to get changed files:", e);
      files = [];
    }
    loading = false;
  }

  async function loadAllDiffs() {
    if (!viewer) return;
    // Use preloaded patches if available (populated when agent finishes)
    const preloaded = getPreloadedPatches(sessionId);

    const patches = await Promise.all(
      files.map(async (f) => {
        const cached = preloaded?.get(f.path);
        if (cached) return cached;
        return git.getFilePatch(repoPath, baseBranch, f.path, f.old_path ?? null).catch(() => "");
      })
    );

    if (preloaded) clearPreloadedPatches(sessionId);

    const items: CodeViewItem<ReviewComment>[] = [];
    for (let i = 0; i < files.length; i++) {
      const patch = patches[i];
      if (!patch) continue;
      const parsed = parsePatchFiles(patch, `${sessionId}-${i}`);
      const fileDiff = parsed[0]?.files[0];
      if (!fileDiff) continue;
      const annotations = getAnnotationsForFile(files[i].path);
      items.push({
        id: `diff:${files[i].path}`,
        type: "diff",
        fileDiff: fileDiff as FileDiffMetadata,
        annotations,
      });
    }
    viewer.setItems(items);
    if (items.length > 0) {
      viewer.scrollTo({ type: "item", id: currentFileId(), align: "start" });
    }
  }

  function getAnnotationsForFile(filePath: string): DiffLineAnnotation<ReviewComment>[] {
    return getComments(sessionId)
      .filter((c) => c.filePath === filePath && c.type !== "file")
      .map((c) => ({ side: "additions" as const, lineNumber: c.startLine, metadata: c }));
  }

  function selectFile(index: number) {
    selectedIndex = index;
    showCommentInput = false;
    cursorLine = 1;
    selectionAnchor = null;
    onFileChange?.(files[index]?.path.split("/").pop() || files[index]?.path || "");
    viewer?.scrollTo({ type: "item", id: `diff:${files[index]?.path}`, align: "start" });
  }

  function navigateFile(index: number) {
    selectFile(index);
    if (diffFocus === "body") {
      cursorLine = 1;
      viewer?.setSelectedLines({ id: currentFileId(), range: { start: 1, end: 1, side: "additions" } });
    }
  }

  // ─── Comments ───────────────────────────────────────────────────────────────

  function openCommentInput(start: number, end: number, type: "line" | "hunk" | "file") {
    commentStartLine = start;
    commentEndLine = end;
    commentType = type;
    commentText = "";
    showCommentInput = true;
    requestAnimationFrame(() => commentInputEl?.focus());
  }

  function submitComment() {
    const text = commentText.trim();
    if (!text) return;
    addComment(sessionId, {
      filePath: files[selectedIndex]?.path ?? "",
      type: commentType,
      startLine: commentStartLine,
      endLine: commentEndLine,
      text,
    });
    cancelComment();
    clearSelection();
    updateAnnotations();
  }

  function cancelComment() {
    showCommentInput = false;
    commentText = "";
  }

  function handleCommentKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); cancelComment(); }
    else if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submitComment(); }
  }

  function updateAnnotations() {
    if (!viewer || files.length === 0) return;
    const file = files[selectedIndex];
    const item = viewer.getItem(`diff:${file.path}`);
    if (item?.type === "diff") {
      viewer.updateItem({
        ...item,
        version: (item.version ?? 0) + 1,
        annotations: getAnnotationsForFile(file.path),
      });
    }
  }

  async function sendFeedback() {
    const comments = getComments(sessionId);
    if (comments.length === 0 || sessionExited) return;
    const filePaths = [...new Set(comments.map((c) => c.filePath))];
    const fileDiffs = new Map<string, FileDiffData>();
    await Promise.all(filePaths.map(async (path) => {
      try { const diff = await git.getFileDiff(repoPath, baseBranch, path, null); fileDiffs.set(path, diff); } catch {}
    }));
    const serialized = serializeComments(comments, fileDiffs);
    const bytes = Array.from(new TextEncoder().encode(serialized));
    await pty.write(sessionId, bytes);
    await pty.write(sessionId, [0x0d]);
    const count = comments.length;
    clearComments(sessionId);
    updateAnnotations();
    showSnackbar(`Feedback sent (${count} comment${count !== 1 ? "s" : ""})`, "success");
  }

  // ─── Selection ──────────────────────────────────────────────────────────────

  function showCursor() {
    viewer?.setSelectedLines({ id: currentFileId(), range: { start: cursorLine, end: cursorLine, side: "additions" } });
  }

  function moveCursor(delta: number) {
    cursorLine = Math.max(1, cursorLine + delta);
    if (selectionAnchor !== null) {
      const start = Math.min(selectionAnchor, cursorLine);
      const end = Math.max(selectionAnchor, cursorLine);
      viewer?.setSelectedLines({ id: currentFileId(), range: { start, end, side: "additions" } });
    } else {
      showCursor();
    }
  }

  function toggleSelectionMode() {
    if (selectionAnchor !== null) { selectionAnchor = null; }
    else { selectionAnchor = cursorLine; }
  }

  function clearSelection() {
    selectionAnchor = null;
    viewer?.setSelectedLines(null);
  }

  // ─── Keyboard ───────────────────────────────────────────────────────────────

  function handleKeydown(e: KeyboardEvent) {
    if (!visible || getActiveZone() !== "terminal") return;
    if (e.key === "Enter" && e.metaKey) { e.preventDefault(); sendFeedback(); return; }
    if (showCommentInput) return;

    // Global keys (both modes)
    if (e.key === "?" || (e.key === "/" && e.shiftKey)) { e.preventDefault(); showHelp = !showHelp; return; }
    if ((e.key === "n" && e.ctrlKey) || (e.key === "ArrowDown" && e.ctrlKey)) {
      e.preventDefault();
      if (selectedIndex < files.length - 1) navigateFile(selectedIndex + 1);
      return;
    }
    if ((e.key === "p" && e.ctrlKey) || (e.key === "ArrowUp" && e.ctrlKey)) {
      e.preventDefault();
      if (selectedIndex > 0) navigateFile(selectedIndex - 1);
      return;
    }
    if (e.key === "]" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      viewer?.scrollTo({ type: "position", position: (viewerRoot?.scrollTop ?? 0) + 200 });
      return;
    }
    if (e.key === "[" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      viewer?.scrollTo({ type: "position", position: Math.max(0, (viewerRoot?.scrollTop ?? 0) - 200) });
      return;
    }
    if (e.key === "u" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); toggleDiffStyle(); return; }
    if (e.key === "r" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); refresh(); return; }
    if (e.key === "e" && !e.metaKey && !e.ctrlKey && files.length > 0) { e.preventDefault(); onEditFile?.(files[selectedIndex].path); return; }

    // List mode
    if (diffFocus === "list") {
      if (e.key === "ArrowDown" || (e.key === "j" && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        if (selectedIndex < files.length - 1) selectFile(selectedIndex + 1);
      } else if (e.key === "ArrowUp" || (e.key === "k" && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        if (selectedIndex > 0) selectFile(selectedIndex - 1);
      } else if (e.key === "Enter" && !e.metaKey) {
        e.preventDefault();
        diffFocus = "body";
        cursorLine = 1;
        showCursor();
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (showHelp) showHelp = false;
      }
      return;
    }

    // Body mode
    if (e.key === "Escape") {
      e.preventDefault();
      if (showHelp) { showHelp = false; return; }
      if (selectionAnchor !== null) { clearSelection(); return; }
      diffFocus = "list";
      clearSelection();
      return;
    }
    if (e.key === "ArrowDown" && e.shiftKey) { e.preventDefault(); if (selectionAnchor === null) selectionAnchor = cursorLine; moveCursor(1); }
    else if (e.key === "ArrowUp" && e.shiftKey) { e.preventDefault(); if (selectionAnchor === null) selectionAnchor = cursorLine; moveCursor(-1); }
    else if (e.key === "ArrowDown" || (e.key === "j" && !e.ctrlKey && !e.metaKey && !e.shiftKey)) { e.preventDefault(); moveCursor(1); }
    else if (e.key === "ArrowUp" || (e.key === "k" && !e.ctrlKey && !e.metaKey && !e.shiftKey)) { e.preventDefault(); moveCursor(-1); }
    else if (e.key === "v" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); toggleSelectionMode(); }
    else if (e.key === "d" && !e.metaKey && !e.ctrlKey || e.key === "f" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); moveCursor(15); }
    else if (e.key === "b" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); moveCursor(-15); }
    else if (e.key === "g" && !e.metaKey && !e.ctrlKey && !e.shiftKey) { e.preventDefault(); cursorLine = 1; showCursor(); }
    else if (e.key === "G" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); cursorLine = 9999; showCursor(); }
    else if (e.key === "c" && !e.metaKey && !e.ctrlKey && files.length > 0) {
      e.preventDefault();
      if (selectionAnchor !== null) {
        const start = Math.min(selectionAnchor, cursorLine);
        const end = Math.max(selectionAnchor, cursorLine);
        openCommentInput(start, end, start === end ? "line" : "hunk");
      } else {
        openCommentInput(cursorLine, cursorLine, "line");
      }
    }
  }

  function toggleDiffStyle() {
    diffStyle = diffStyle === "split" ? "unified" : "split";
    // Rebuild with new style — CodeView options are set at construction, so we recreate
    if (viewer && viewerRoot) {
      viewer.cleanUp();
      viewer = createViewer();
      refresh();
    }
  }

  function createViewer(): CodeView {
    const v = new CodeView({
      theme: { dark: "github-dark", light: "github-light" },
      themeType: isDark() ? "dark" : "light",
      diffStyle,
      stickyHeaders: true,
      enableLineSelection: true,
      disableFileHeader: false,
      onLineSelected(range) {
        if (range) {
          diffFocus = "body";
          cursorLine = range.start;
        }
      },
      renderAnnotation(annotation) {
        const comment = annotation.metadata as ReviewComment | undefined;
        if (!comment) return undefined;
        const el = document.createElement("div");
        el.style.cssText = "padding:6px 10px;margin:2px 0;border-radius:4px;font-size:12px;line-height:1.4;display:flex;align-items:flex-start;gap:8px;background:var(--comment-bg,rgba(128,128,128,0.1));border:1px solid var(--comment-border,rgba(128,128,128,0.2))";
        const text = document.createElement("span");
        text.style.cssText = "flex:1;white-space:pre-wrap;word-break:break-word";
        text.textContent = comment.text;
        const del = document.createElement("button");
        del.style.cssText = "background:none;border:none;cursor:pointer;padding:2px;color:#888;font-size:14px";
        del.textContent = "×";
        del.onclick = () => { removeComment(sessionId, comment.id); updateAnnotations(); };
        el.appendChild(text);
        el.appendChild(del);
        return el;
      },
      layout: { paddingTop: 8, paddingBottom: 8, gap: 0 },
    }, getWorkerPool());
    v.setup(viewerRoot);
    return v;
  }

  // ─── Lifecycle ──────────────────────────────────────────────────────────────

  onMount(() => {
    window.addEventListener("keydown", handleKeydown);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeydown);
    viewer?.cleanUp();
    viewer = null;
  });

  $effect(() => {
    if (visible && !mounted && viewerRoot) {
      mounted = true;
      viewer = createViewer();
      refresh();
    }
  });

  $effect(() => {
    const dark = isDark();
    if (viewer && mounted) {
      viewer.setOptions({ themeType: dark ? "dark" : "light" });
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

  function fileName(path: string): string { return path.split("/").pop() || path; }
  function dirName(path: string): string { const parts = path.split("/"); return parts.length > 1 ? parts.slice(0, -1).join("/") + "/" : ""; }
</script>

<div class="flex h-full w-full" class:hidden={!visible}>
  <div class="flex-1 min-w-0 relative overflow-hidden">
    <!-- Toolbar -->
    <div class="absolute top-0 left-0 right-0 h-8 flex items-center px-3 gap-2 border-b border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 z-10">
      <button class="text-xs px-2 py-0.5 rounded {diffStyle === 'split' ? 'bg-primary-100 dark:bg-primary-900 text-primary-700 dark:text-primary-300' : 'text-surface-500 hover:text-surface-700 dark:hover:text-surface-400'}" onclick={() => { diffStyle = "split"; toggleDiffStyle(); }}>Split</button>
      <button class="text-xs px-2 py-0.5 rounded {diffStyle === 'unified' ? 'bg-primary-100 dark:bg-primary-900 text-primary-700 dark:text-primary-300' : 'text-surface-500 hover:text-surface-700 dark:hover:text-surface-400'}" onclick={() => { diffStyle = "unified"; toggleDiffStyle(); }}>Unified</button>
      <div class="flex-1"></div>
      {#if files.length > 0}
        <button class="text-xs px-2 py-0.5 rounded text-surface-500 hover:text-surface-700 dark:hover:text-surface-400 hover:bg-surface-200 dark:hover:bg-surface-700" onclick={() => openCommentInput(0, 0, "file")} title="Add file-level comment"><MessageSquare size={12} /></button>
      {/if}
      {#if totalCount > 0}
        <Button variant="primary" size="sm" onclick={sendFeedback} disabled={sessionExited} title={sessionExited ? "Agent is not running" : "Send feedback (⌘Enter)"}><Send size={12} /><span class="ml-1">Send ({totalCount})</span></Button>
      {/if}
      <button class="text-xs px-1.5 py-0.5 rounded text-surface-400 hover:text-surface-600 dark:hover:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-700" onclick={() => (showHelp = !showHelp)} title="Keyboard shortcuts (?)">?</button>
    </div>

    <!-- Comment input -->
    {#if showCommentInput}
      <div class="absolute top-8 left-0 right-0 z-20 p-2 border-b border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-800">
        {#if commentType !== "file"}
          <div class="text-[10px] text-surface-500 mb-1">Comment on {commentType === "hunk" ? `lines ${commentStartLine}–${commentEndLine}` : `line ${commentStartLine}`}</div>
        {/if}
        <textarea bind:this={commentInputEl} bind:value={commentText} onkeydown={handleCommentKeydown} class="w-full p-2 text-xs rounded border border-surface-300 dark:border-surface-600 bg-white dark:bg-surface-900 text-surface-900 dark:text-surface-100 resize-none focus:outline-none focus:ring-1 focus:ring-primary-500" rows="3" placeholder={commentType === "file" ? "File-level comment… (Enter to submit, Esc to cancel)" : "Add comment… (Enter to submit, Esc to cancel)"}></textarea>
      </div>
    {/if}

    <!-- CodeView container -->
    <div bind:this={viewerRoot} class="absolute inset-0 top-8 overflow-auto {diffFocus === 'body' ? 'ring-1 ring-primary-400/40 ring-inset' : ''}"></div>

    <!-- Loading states -->
    {#if loading && files.length === 0}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">Loading diff…</div>
    {:else if files.length === 0 && !loading}
      <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">No changes on this branch</div>
    {/if}

    <!-- Help dialog -->
    {#if showHelp}
      <Dialog.Root open={showHelp} onOpenChange={(v) => (showHelp = v)}>
        <Dialog.Portal>
          <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-full max-w-xs -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-200 bg-surface-50 p-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none">
            <Dialog.Title class="text-sm font-medium text-surface-900 dark:text-surface-50 mb-3">Review Shortcuts</Dialog.Title>
            <div class="text-xs text-surface-500 uppercase tracking-wide mb-1">List</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs text-surface-700 dark:text-surface-300 mb-2">
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">j/k</kbd><span>Navigate files</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Enter</kbd><span>Focus diff</span>
            </div>
            <div class="text-xs text-surface-500 uppercase tracking-wide mb-1">Body</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs text-surface-700 dark:text-surface-300 mb-2">
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">j/k</kbd><span>Move cursor</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Shift+↓/↑</kbd><span>Select</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">v</kbd><span>Visual select</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">d/f b</kbd><span>Page ↓/↑</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">g/G</kbd><span>Top/bottom</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">c</kbd><span>Comment</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Esc</kbd><span>Back to list</span>
            </div>
            <div class="text-xs text-surface-500 uppercase tracking-wide mb-1">Both</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs text-surface-700 dark:text-surface-300">
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">Ctrl+n/p</kbd><span>Next/prev file</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">]/[</kbd><span>Scroll hunks</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">u</kbd><span>Split/unified</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">e</kbd><span>Edit file</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">r</kbd><span>Refresh</span>
              <kbd class="font-mono bg-surface-200 dark:bg-surface-700 px-1 rounded">⌘↵</kbd><span>Send feedback</span>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}
  </div>

  <!-- File sidebar -->
  <div class="relative shrink-0 border-l border-surface-200 dark:border-surface-800 bg-surface-50 dark:bg-surface-900 overflow-y-auto transition-opacity {diffFocus === 'body' ? 'opacity-60' : ''}" style:width="{sidebarWidth}px">
    <ResizeHandle side="left" bind:width={sidebarWidth} min={180} max={Infinity} defaultWidth={256} onResizeEnd={(w) => setLayoutWidth("diff-sidebar", w)} />
    <div class="px-3 py-2 text-xs font-medium text-surface-500 dark:text-surface-400 uppercase tracking-wider border-b border-surface-200 dark:border-surface-800">Changed files ({files.length})</div>
    <ul class="py-1" role="listbox">
      {#each files as file, i (file.path)}
        {@const fileCount = getFileCommentCount(sessionId, file.path)}
        <li role="option" aria-selected={i === selectedIndex} class="px-2 py-1 cursor-pointer flex items-center gap-1 text-xs text-surface-700 dark:text-surface-200 hover:bg-surface-100 dark:hover:bg-surface-800 {i === selectedIndex ? 'bg-surface-200 dark:bg-surface-700' : ''}" onclick={() => selectFile(i)}>
          <span class="font-mono w-4 shrink-0 {statusColor(file.status)}">{file.status}</span>
          <span class="truncate flex-1" title={file.path}><span class="text-surface-400">{dirName(file.path)}</span>{fileName(file.path)}</span>
          {#if fileCount > 0}<span class="flex items-center gap-0.5 text-[10px] text-primary-600 dark:text-primary-400"><MessageSquare size={10} />{fileCount}</span>{/if}
          <span class="text-green-600 dark:text-green-300 text-[10px]">+{file.additions}</span>
          <span class="text-red-600 dark:text-red-300 text-[10px]">-{file.deletions}</span>
        </li>
      {/each}
    </ul>
  </div>
</div>
