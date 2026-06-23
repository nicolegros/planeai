<script lang="ts">
  import { git } from "../lib/api";
  import type { ChangedFile, FileDiff as FileDiffData } from "../lib/types";
  import { onMount, onDestroy } from "svelte";
  import { CodeView, parsePatchFiles, type CodeViewItem, type DiffLineAnnotation, type SelectedLineRange, type FileDiffMetadata } from "@pierre/diffs";
  import { getOrCreateWorkerPoolSingleton, terminateWorkerPoolSingleton } from "@pierre/diffs/worker";
  import { workerFactory } from "../lib/worker-factory";
  import { isDark, getSettings } from "../lib/settings.svelte";
  import { getActiveZone } from "../lib/focus.svelte";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { ResizeHandle } from "./ui";
  import { addComment, removeComment, editComment, getComments, getFileCommentCount, getTotalCommentCount, clearComments, type ReviewComment } from "../lib/review-comments.svelte";
  import { MessageSquare, Send, Check } from "@lucide/svelte";
  import { pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { serializeComments } from "../lib/review-serializer";
  import { getActiveSession } from "../lib/session-orchestrator.svelte";
  import Button from "./ui/Button.svelte";
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
  let diffFocus = $state<"list" | "body">("list");
  let commentText = $state("");
  let commentStartLine = $state(0);
  let commentEndLine = $state(0);
  let commentType = $state<"line" | "hunk" | "file">("line");
  let cursorLine = $state(1);
  let selectionAnchor = $state<number | null>(null);
  let commentInputEl = $state<HTMLTextAreaElement | undefined>();
  let editingCommentId = $state<string | null>(null);

  // CodeView + Worker Pool
  let viewerRoot: HTMLElement;
  let viewer: CodeView<ReviewComment> | null = null;
  let mounted = false;

  // Hunk navigation state: array of { fileIndex, line } for each hunk's first changed line (additions side)
  let hunkPositions: { fileIndex: number; line: number }[] = [];
  // Visible line ranges per file index: ranges the cursor can occupy
  let visibleRanges: Map<number, { start: number; end: number }[]> = new Map();

  // Viewed files state
  let viewedFiles = $state<Set<string>>(new Set());
  let patchFingerprints = new Map<string, string>();
  let allItems: CodeViewItem<ReviewComment>[] = [];
  let viewedVersion = 0;

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
  let currentFileComments = $derived(
    getComments(sessionId).filter((c) => c.filePath === (files[selectedIndex]?.path ?? ""))
  );


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

    let patches: string[];
    if (preloaded && preloaded.size >= files.length) {
      patches = files.map((f) => preloaded.get(f.path) ?? "");
    } else {
      // Single batch IPC call — one invoke, one deserialization
      const fileArgs: [string, string | null][] = files.map((f) => [f.path, f.old_path ?? null]);
      patches = await git.getAllFilePatches(repoPath, baseBranch, fileArgs);
    }

    if (!viewer) return;
    if (preloaded) clearPreloadedPatches(sessionId);

    // Update fingerprints and invalidate viewed state for changed files
    const newFingerprints = new Map<string, string>();
    for (let i = 0; i < files.length; i++) {
      const fp = `${files[i].additions}:${files[i].deletions}:${(patches[i] || "").length}`;
      newFingerprints.set(files[i].path, fp);
      if (viewedFiles.has(files[i].path) && patchFingerprints.get(files[i].path) !== fp) {
        viewedFiles.delete(files[i].path);
        viewedFiles = new Set(viewedFiles);
      }
    }
    patchFingerprints = newFingerprints;

    const items: CodeViewItem<ReviewComment>[] = [];
    for (let i = 0; i < files.length; i++) {
      const patch = patches[i];
      if (!patch) continue;
      const parsed = parsePatchFiles(patch, `${sessionId}-${i}`);
      const fileDiff = parsed[0]?.files[0];
      if (!fileDiff) continue;
      delete fileDiff.cacheKey;
      const annotations = getAnnotationsForFile(files[i].path);
      items.push({
        id: `diff:${files[i].path}`,
        type: "diff",
        fileDiff: fileDiff as FileDiffMetadata,
        annotations,
      });
    }
    allItems = items;
    viewer.setItems(allItems.map((it) => ({ ...it, collapsed: viewedFiles.has(it.id.replace("diff:", "")) })));
    computeHunkMeta(items);
    if (items.length > 0 && viewer.getItem(currentFileId())) {
      viewer.scrollTo({ type: "item", id: currentFileId(), align: "start" });
    }
  }

  function computeHunkMeta(items: CodeViewItem<ReviewComment>[]) {
    const positions: { fileIndex: number; line: number }[] = [];
    const ranges = new Map<number, { start: number; end: number }[]>();

    for (let fi = 0; fi < items.length; fi++) {
      const item = items[fi];
      if (item.type !== "diff") continue;
      // Map item back to files[] index
      const filesIdx = files.findIndex((f) => `diff:${f.path}` === item.id);
      const fileRanges: { start: number; end: number }[] = [];
      for (const hunk of item.fileDiff.hunks) {
        // Find the first changed line in this hunk
        let firstChangeLine = hunk.additionStart;
        for (const seg of hunk.hunkContent) {
          if (seg.type === "change") {
            firstChangeLine = hunk.additionStart + (seg.additionLineIndex - hunk.additionLineIndex);
            break;
          }
        }
        positions.push({ fileIndex: filesIdx, line: firstChangeLine });

        // Visible range for this hunk: additionStart to additionStart + additionCount - 1
        fileRanges.push({ start: hunk.additionStart, end: hunk.additionStart + hunk.additionCount - 1 });
      }
      ranges.set(filesIdx, fileRanges);
    }
    hunkPositions = positions;
    visibleRanges = ranges;
  }

  function getAnnotationsForFile(filePath: string): DiffLineAnnotation<ReviewComment>[] {
    return getComments(sessionId)
      .filter((c) => c.filePath === filePath && c.type !== "file")
      .map((c) => ({ side: "additions" as const, lineNumber: c.startLine, metadata: c }));
  }

  function selectFile(index: number) {
    selectedIndex = index;
    showCommentInput = false;
    selectionAnchor = null;
    onFileChange?.(files[index]?.path.split("/").pop() || files[index]?.path || "");
    cursorLine = snapToVisible(1, 1);
    const id = `diff:${files[index]?.path}`;
    if (viewer?.getItem(id)) viewer.scrollTo({ type: "item", id, align: "start" });
    if (diffFocus === "body") showCursor();
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
    if (editingCommentId) {
      editComment(sessionId, editingCommentId, text);
    } else {
      addComment(sessionId, {
        filePath: files[selectedIndex]?.path ?? "",
        type: commentType,
        startLine: commentStartLine,
        endLine: commentEndLine,
        text,
      });
    }
    cancelComment();
    clearSelection();
    updateAnnotations();
  }

  function cancelComment() {
    showCommentInput = false;
    commentText = "";
    editingCommentId = null;
  }

  function openEditComment(comment: ReviewComment) {
    commentStartLine = comment.startLine;
    commentEndLine = comment.endLine;
    commentType = comment.type;
    commentText = comment.text;
    editingCommentId = comment.id;
    showCommentInput = true;
    requestAnimationFrame(() => commentInputEl?.focus());
  }

  function handleCommentKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); cancelComment(); }
    else if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submitComment(); }
  }

  function updateAnnotations() {
    if (!viewer || files.length === 0) return;
    allItems = allItems.map((it) =>
      it.type === "diff"
        ? { ...it, version: (it.version ?? 0) + 1, annotations: getAnnotationsForFile(it.id.replace("diff:", "")) }
        : it,
    );
    viewer.setItems(allItems.map((it) => ({ ...it, collapsed: viewedFiles.has(it.id.replace("diff:", "")) })));
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

  // ─── Viewed ──────────────────────────────────────────────────────────────────

  function toggleViewed(index: number, advance = false) {
    const path = files[index]?.path;
    if (!path) return;
    const wasViewed = viewedFiles.has(path);
    if (wasViewed) { viewedFiles.delete(path); } else { viewedFiles.add(path); }
    viewedFiles = new Set(viewedFiles);
    // Update CodeView items (collapse/uncollapse viewed files)
    viewedVersion++;
    viewer?.setItems(allItems.map((it) => ({ ...it, collapsed: viewedFiles.has(it.id.replace("diff:", "")), version: viewedVersion })));
    // Auto-advance to next unviewed file when marking as viewed
    if (advance && !wasViewed) {
      const next = files.findIndex((f, i) => i > index && !viewedFiles.has(f.path));
      if (next !== -1) { selectFile(next); return; }
      const wrap = files.findIndex((f) => !viewedFiles.has(f.path));
      if (wrap !== -1) { selectFile(wrap); return; }
    }
  }

  // ─── Selection ──────────────────────────────────────────────────────────────

  function navigateHunk(direction: 1 | -1) {
    if (hunkPositions.length === 0) return;
    // Find next hunk relative to current file + cursor position
    let targetIdx = -1;
    if (direction === 1) {
      targetIdx = hunkPositions.findIndex((h) => h.fileIndex > selectedIndex || (h.fileIndex === selectedIndex && h.line > cursorLine));
      if (targetIdx === -1) targetIdx = 0; // wrap
    } else {
      for (let i = hunkPositions.length - 1; i >= 0; i--) {
        if (hunkPositions[i].fileIndex < selectedIndex || (hunkPositions[i].fileIndex === selectedIndex && hunkPositions[i].line < cursorLine)) {
          targetIdx = i;
          break;
        }
      }
      if (targetIdx === -1) targetIdx = hunkPositions.length - 1; // wrap
    }
    const target = hunkPositions[targetIdx];
    if (target.fileIndex !== selectedIndex) {
      selectedIndex = target.fileIndex;
      onFileChange?.(files[selectedIndex]?.path.split("/").pop() || files[selectedIndex]?.path || "");
    }
    diffFocus = "body";
    cursorLine = target.line;
    selectionAnchor = null;
    const id = `diff:${files[selectedIndex]?.path}`;
    viewer?.scrollTo({ type: "line", id, lineNumber: cursorLine, side: "additions", align: "center" });
    viewer?.setSelectedLines({ id, range: { start: cursorLine, end: cursorLine, side: "additions" } });
  }

  function showCursor() {
    const id = currentFileId();
    if (!id) return;
    const ranges = visibleRanges.get(selectedIndex);
    if (ranges && ranges.length > 0) {
      const inRange = ranges.some((r) => cursorLine >= r.start && cursorLine <= r.end);
      if (!inRange) cursorLine = ranges[0].start;
    }
    viewer?.setSelectedLines({ id, range: { start: cursorLine, end: cursorLine, side: "additions" } });
    // Scroll the selected line into view directly on the viewerRoot container
    if (viewerRoot) {
      for (const host of viewerRoot.querySelectorAll("diffs-container")) {
        const el = host.shadowRoot?.querySelector("[data-selected-line]") as HTMLElement | null;
        if (!el) continue;
        const containerRect = viewerRoot.getBoundingClientRect();
        const elRect = el.getBoundingClientRect();
        const margin = 40;
        if (elRect.bottom + margin > containerRect.bottom) {
          viewerRoot.scrollTop += elRect.bottom - containerRect.bottom + margin;
        } else if (elRect.top - margin < containerRect.top) {
          viewerRoot.scrollTop -= containerRect.top - elRect.top + margin;
        }
        break;
      }
    }
  }

  function moveCursor(delta: number) {
    const prevLine = cursorLine;
    const newLine = Math.max(1, cursorLine + delta);
    cursorLine = snapToVisible(newLine, delta >= 0 ? 1 : -1);
    // Cross-file advancement when stuck at boundary
    if (cursorLine === prevLine && selectionAnchor === null) {
      const dir = delta >= 0 ? 1 : -1;
      const nextIdx = findNextFile(selectedIndex, dir);
      if (nextIdx !== -1) {
        selectedIndex = nextIdx;
        onFileChange?.(files[nextIdx]?.path.split("/").pop() || files[nextIdx]?.path || "");
        diffFocus = "body";
        const ranges = visibleRanges.get(nextIdx);
        if (ranges && ranges.length > 0) {
          cursorLine = dir === 1 ? ranges[0].start : ranges[ranges.length - 1].end;
        }
        showCursor();
        return;
      }
    }
    if (selectionAnchor !== null) {
      const start = Math.min(selectionAnchor, cursorLine);
      const end = Math.max(selectionAnchor, cursorLine);
      try { viewer?.setSelectedLines({ id: currentFileId(), range: { start, end, side: "additions" } }); } catch {}
    } else {
      showCursor();
    }
  }

  function findNextFile(fromIndex: number, direction: 1 | -1): number {
    for (let i = fromIndex + direction; i >= 0 && i < files.length; i += direction) {
      if (!viewedFiles.has(files[i].path)) return i;
    }
    // All remaining are viewed — advance to adjacent anyway
    const next = fromIndex + direction;
    return next >= 0 && next < files.length ? next : -1;
  }

  function snapToVisible(line: number, direction: 1 | -1): number {
    const fileRanges = visibleRanges.get(selectedIndex);
    if (!fileRanges || fileRanges.length === 0) return line;
    // Check if line falls within any visible range
    for (const r of fileRanges) {
      if (line >= r.start && line <= r.end) return line;
    }
    // Line is in a folded region — find the nearest visible line in the given direction
    if (direction === 1) {
      for (const r of fileRanges) {
        if (r.start > line) return r.start;
      }
      return fileRanges[fileRanges.length - 1].end; // clamp to last
    } else {
      for (let i = fileRanges.length - 1; i >= 0; i--) {
        if (fileRanges[i].end < line) return fileRanges[i].end;
      }
      return fileRanges[0].start; // clamp to first
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
    if (e.key === "?" || (e.key === "/" && e.shiftKey)) {
      e.preventDefault();
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", metaKey: true, bubbles: true }));
      return;
    }
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
      navigateHunk(1);
      return;
    }
    if (e.key === "[" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      navigateHunk(-1);
      return;
    }
    if (e.key === "u" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); toggleDiffStyle(); return; }
    if (e.key === "m" && !e.metaKey && !e.ctrlKey && files.length > 0) { e.preventDefault(); toggleViewed(selectedIndex, true); return; }
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
        cursorLine = snapToVisible(1, 1);
        showCursor();
      } else if (e.key === "Escape") {
        e.preventDefault();
      }
      return;
    }

    // Body mode
    if (e.key === "Escape") {
      e.preventDefault();
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
    else if (e.key === "g" && !e.metaKey && !e.ctrlKey && !e.shiftKey) { e.preventDefault(); cursorLine = snapToVisible(1, 1); showCursor(); }
    else if (e.key === "G" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); cursorLine = snapToVisible(9999, -1); showCursor(); }
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
    else if (e.key === "E" && !e.metaKey && !e.ctrlKey && files.length > 0) {
      e.preventDefault();
      const comment = getComments(sessionId).find((c) => c.filePath === files[selectedIndex]?.path && c.startLine <= cursorLine && c.endLine >= cursorLine);
      if (comment) openEditComment(comment);
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

  function createViewer(): CodeView<ReviewComment> {
    const v = new CodeView<ReviewComment>({
      theme: { dark: "github-dark", light: "github-light" },
      themeType: isDark() ? "dark" : "light",
      diffStyle,
      stickyHeaders: false,
      enableLineSelection: true,
      disableVirtualizationBuffers: true,
      disableFileHeader: false,
      renderHeaderMetadata(fileDiff) {
        const path = fileDiff.name;
        const idx = files.findIndex((f) => f.path === path);
        if (idx === -1) return null;
        const btn = document.createElement("button");
        btn.title = "Mark as viewed (m)";
        btn.style.cssText = "background:none;border:none;cursor:pointer;padding:2px 4px;border-radius:4px;display:flex;align-items:center;line-height:1";
        const updateBtn = () => {
          btn.innerHTML = viewedFiles.has(path)
            ? `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="color:#16a34a"><polyline points="20 6 9 17 4 12"></polyline></svg>`
            : `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="color:#888"><circle cx="12" cy="12" r="10"></circle></svg>`;
        };
        updateBtn();
        btn.onclick = (e) => { e.stopPropagation(); toggleViewed(idx); updateBtn(); };
        return btn;
      },
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
        const edit = document.createElement("button");
        edit.style.cssText = "background:none;border:none;cursor:pointer;padding:2px;color:#888;font-size:12px";
        edit.textContent = "✎";
        edit.title = "Edit comment";
        edit.onclick = () => { openEditComment(comment); };
        const del = document.createElement("button");
        del.style.cssText = "background:none;border:none;cursor:pointer;padding:2px;color:#888;font-size:14px";
        del.textContent = "×";
        del.onclick = () => { removeComment(sessionId, comment.id); updateAnnotations(); };
        el.appendChild(text);
        el.appendChild(edit);
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
    window.addEventListener("keyup", handleKeyup);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeydown);
    window.removeEventListener("keyup", handleKeyup);
    viewer?.cleanUp();
    viewer = null;
  });

  function handleKeyup(e: KeyboardEvent) {
    if (e.key === "Shift" && selectionAnchor !== null && diffFocus === "body") {
      selectionAnchor = null;
    }
  }

  $effect(() => {
    if (visible && !mounted && viewerRoot) {
      mounted = true;
      viewer = createViewer();
      applyDiffFont();
      refresh();
    }
  });

  function applyDiffFont() {
    if (!viewerRoot) return;
    const { font_family, font_size } = getSettings().terminal;
    viewerRoot.style.setProperty("--diffs-font-family", `"${font_family}", monospace`);
    viewerRoot.style.setProperty("--diffs-font-size", `${font_size}px`);
  }

  // Keep the diff font (family + size) in sync with the terminal font settings,
  // so changing it in Preferences updates the diff without a remount.
  $effect(() => {
    const { font_family, font_size } = getSettings().terminal;
    void font_family;
    void font_size;
    if (mounted) applyDiffFont();
  });

  $effect(() => {
    const dark = isDark();
    if (viewer && mounted) {
      viewer.setOptions({ themeType: dark ? "dark" : "light" });
    }
  });

  function statusColor(status: string): string {
    switch (status) {
      case "A": return "text-status-running";
      case "D": return "text-status-exited";
      case "M": return "text-status-review";
      case "R": return "text-accent";
      default: return "text-t3";
    }
  }

  function fileName(path: string): string { return path.split("/").pop() || path; }
  function dirName(path: string): string { const parts = path.split("/"); return parts.length > 1 ? parts.slice(0, -1).join("/") + "/" : ""; }
</script>

<div class="flex h-full w-full" class:hidden={!visible}>
  <div class="flex-1 min-w-0 relative overflow-hidden">
    <!-- Toolbar -->
    <div class="absolute top-0 left-0 right-0 h-[42px] flex items-center px-4 gap-3 border-b border-border bg-chrome z-10">
      {#if files.length > 0}
        <span class="font-mono text-[12px] text-t1 truncate">{files[selectedIndex]?.path ?? ''}</span>
        <span class="text-[11px] text-status-running">+{files[selectedIndex]?.additions ?? 0}</span>
        <span class="text-[11px] text-status-exited">−{files[selectedIndex]?.deletions ?? 0}</span>
      {/if}
      <div class="flex-1"></div>
      <div class="flex gap-0.5 rounded-lg bg-panel-hi p-0.5">
        <button class="px-2 py-1 text-[11px] rounded-md transition-colors {diffStyle === 'unified' ? 'bg-accent-bg text-accent' : 'text-t2 hover:text-t1'}" onclick={() => { diffStyle = "unified"; toggleDiffStyle(); }}>Unified</button>
        <button class="px-2 py-1 text-[11px] rounded-md transition-colors {diffStyle === 'split' ? 'bg-accent-bg text-accent' : 'text-t2 hover:text-t1'}" onclick={() => { diffStyle = "split"; toggleDiffStyle(); }}>Split</button>
      </div>
      {#if files.length > 0}
        <span class="font-mono text-[10px] text-t3 bg-panel-hi px-1.5 py-0.5 rounded">c Comment</span>
      {/if}
      {#if totalCount > 0}
        <Button variant="primary" size="sm" onclick={sendFeedback} disabled={sessionExited} title={sessionExited ? "Agent is not running" : "Send feedback (⌘Enter)"}><Send size={12} /><span class="ml-1">Send ({totalCount})</span></Button>
      {/if}
    </div>

    <!-- Comment input -->
    {#if showCommentInput}
      <div class="absolute top-[42px] left-0 right-0 z-20 p-3 border-b border-border bg-chrome">
        {#if commentType !== "file"}
          <div class="text-[10px] text-t3 mb-1.5">● {editingCommentId ? "Edit note" : "Review note"} · {commentType === "hunk" ? `lines ${commentStartLine}–${commentEndLine}` : `line ${commentStartLine}`}</div>
        {/if}
        <textarea bind:this={commentInputEl} bind:value={commentText} onkeydown={handleCommentKeydown} class="w-full p-2 text-[12.5px] rounded-lg border border-border-s bg-panel text-t1 resize-none focus:outline-none focus:ring-1 focus:ring-accent" rows="3" placeholder="{editingCommentId ? 'Edit note…' : 'Add a note…'} (Enter to submit, Esc to cancel)"></textarea>
        <span class="font-mono text-[10px] text-t3 mt-1 inline-block">r reply</span>
      </div>
    {/if}

    <!-- Comment list (split view) -->
    {#if diffStyle === "split" && currentFileComments.length > 0 && !showCommentInput}
      <div class="absolute top-[42px] left-0 right-0 z-10 border-b border-border bg-chrome max-h-[200px] overflow-y-auto">
        {#each currentFileComments as comment (comment.id)}
          <div class="flex items-start gap-2 px-4 py-2 border-b border-border/50 last:border-b-0">
            <span class="shrink-0 text-[10px] text-t3 font-mono pt-0.5">{comment.type === "hunk" ? `L${comment.startLine}–${comment.endLine}` : `L${comment.startLine}`}</span>
            <span class="flex-1 text-[12px] text-t1 whitespace-pre-wrap break-words">{comment.text}</span>
            <button class="shrink-0 text-[11px] text-t3 hover:text-t1" onclick={() => openEditComment(comment)} title="Edit">✎</button>
            <button class="shrink-0 text-[13px] text-t3 hover:text-t1" onclick={() => { removeComment(sessionId, comment.id); updateAnnotations(); }} title="Delete">×</button>
          </div>
        {/each}
      </div>
    {/if}

    <!-- CodeView container -->
    <div bind:this={viewerRoot} class="absolute inset-0 top-[42px] overflow-auto"></div>

    <!-- Loading states -->
    {#if loading}
      <div class="absolute inset-0 top-[42px] flex items-center justify-center text-t3 bg-main z-[5]">Loading diff…</div>
    {:else if files.length === 0}
      <div class="absolute inset-0 top-[42px] flex items-center justify-center text-t3 bg-main">No changes on this branch</div>
    {/if}

  </div>

  <!-- File sidebar (right) -->
  <div class="relative shrink-0 border-l border-border bg-chrome overflow-y-auto overflow-x-hidden" style:width="{sidebarWidth}px">
    <ResizeHandle side="left" bind:width={sidebarWidth} min={180} max={Infinity} defaultWidth={240} onResizeEnd={(w) => setLayoutWidth("diff-sidebar", w)} />
    <div class="px-3 py-2.5 text-[10px] font-semibold text-t3 uppercase tracking-[.05em] border-b border-border flex items-center justify-between">
      <span>Changed · {files.length}</span>
      <span class="text-status-running">+{files.reduce((a, f) => a + f.additions, 0)}</span>
      <span class="text-status-exited">−{files.reduce((a, f) => a + f.deletions, 0)}</span>
    </div>
    <ul class="py-1" role="listbox">
      {#each files as file, i (file.path)}
        {@const fileCount = getFileCommentCount(sessionId, file.path)}
        {@const viewed = viewedFiles.has(file.path)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li role="option" aria-selected={i === selectedIndex} class="flex items-center gap-1 mx-1 {viewed ? 'opacity-50' : ''}" onclick={() => selectFile(i)}>
          <span class="w-0.5 self-stretch rounded-full transition-opacity {i === selectedIndex ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
          <div class="flex-1 min-w-0 flex items-center gap-1.5 px-2 py-1.5 cursor-pointer text-[12px] rounded-lg {i === selectedIndex ? 'bg-accent-bg' : 'hover:bg-panel-hi'}">
          {#if viewed}
            <button class="shrink-0 text-status-running" onclick={(e) => { e.stopPropagation(); toggleViewed(i); }} title="Mark as unviewed"><Check size={12} /></button>
          {:else}
            <span class="font-mono w-4 shrink-0 text-[10px] {statusColor(file.status)}">{file.status}</span>
          {/if}
          <span class="truncate flex-1 font-mono" title={file.path}><span class="text-t3 text-[9.5px]">{dirName(file.path)}</span><span class="text-t1 text-[12px]">{fileName(file.path)}</span></span>
          {#if fileCount > 0}<span class="flex items-center gap-0.5 text-[10px] text-accent"><MessageSquare size={10} />{fileCount}</span>{/if}
          <span class="text-[10px] font-mono text-t3">+{file.additions} −{file.deletions}</span>
          </div>
        </li>
      {/each}
    </ul>
  </div>
</div>

