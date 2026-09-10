<script lang="ts">
  import { git, pty } from "../lib/api";
  import type { ChangedFile } from "../lib/types";
  import { onDestroy, onMount } from "svelte";
  import { isDark, getSettings } from "../lib/settings.svelte";
  import { getActiveZone } from "../lib/focus.svelte";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { ResizeHandle } from "./ui";
  import { addComment, clearComments, editComment, getComments, getFileCommentCount, getTotalCommentCount, removeComment, type ReviewComment } from "../lib/review-comments.svelte";
  import { ChevronDown, ChevronRight, MessageSquare, Send, Check, AlertTriangle, LoaderCircle } from "@lucide/svelte";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { MOD_ENTER_HINT } from "../lib/keyboard";
  import { serializeComments } from "../lib/review-serializer";
  import { getActiveSession, recordUserInput } from "../lib/session-orchestrator.svelte";
  import Button from "./ui/Button.svelte";
  import { hasConflicts } from "../lib/ci-checks.svelte";
  import BranchCompareForm from "./BranchCompareForm.svelte";
  import { getComparison, setComparison, formatComparison } from "../lib/diff-comparison.svelte";
  import { ensureSession, getViewedFiles, setFileViewed, setFileUnviewed, isFileViewed, invalidateViewedFiles, getViewedVersion } from "../lib/diff-viewed.svelte";
  import CodeMirrorDiff from "./CodeMirrorDiff.svelte";
  import { contentFingerprint, type ReviewFileDiff, type ReviewSelection, type TextFileDiff } from "../lib/review-diff";
  import { expandReviewTreeAncestors, getReviewFolderPaths, getReviewTreeRows, reconcileReviewTreeExpansion, type ReviewTreeRow } from "../lib/diff-sidebar-tree";

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
  let fileLoading = $state(false);
  let diffStyle = $state<"split" | "unified">("split");
  let sidebarWidth = $state(getLayoutWidth("diff-sidebar", 256));
  let showCompareForm = $state(false);
  let diffFocus = $state<"list" | "body">("list");
  let selectedRange = $state<ReviewSelection | null>(null);
  let showCommentInput = $state(false);
  let commentText = $state("");
  let commentType = $state<"line" | "hunk" | "file">("line");
  let commentRange = $state<ReviewSelection | null>(null);
  let editingCommentId = $state<string | null>(null);
  let commentInputEl = $state<HTMLTextAreaElement>();
  let sendingFeedback = $state(false);
  let allFilesReviewed = $state(false);
  let mounted = false;
  let refreshGeneration = 0;
  let sidebarListRef = $state<HTMLElement>();
  let sidebarFocusedRowId = $state<string | null>(null);
  let expandedSidebarFolders = $state<Set<string>>(new Set());
  let sidebarFolderPaths = $state<Set<string>>(new Set());
  let sidebarTreeInitialized = false;
  let sidebarRows = $derived(getReviewTreeRows(files, expandedSidebarFolders));
  let diffView = $state<CodeMirrorDiff>();
  let cache = new Map<string, ReviewFileDiff>();
  let inFlightDiffs = new Map<string, Promise<ReviewFileDiff>>();
  let selectedLoadRequest = 0;
  let activeDiff = $state<ReviewFileDiff | null>(null);

  let comparison = $derived(getComparison(sessionId, baseBranch));
  let effectiveBase = $derived(comparison.baseRef);
  let effectiveHead = $derived(comparison.headRef);
  let viewedFiles = $derived(getViewedFiles(sessionId));
  let viewedVersion = $derived(getViewedVersion(sessionId));
  let totalCount = $derived(getTotalCommentCount(sessionId));
  let sessionExited = $derived(getActiveSession()?.status === "exited");
  let conflicted = $derived(hasConflicts(sessionId));
  let contentTop = $derived(conflicted ? 76 : 42);
  let activeFile = $derived(files[selectedIndex] ?? null);
  let activeTextDiff = $derived(activeDiff?.kind === "text" ? activeDiff : null);
  let currentFileComments = $derived(getComments(sessionId).filter((comment) => comment.filePath === activeFile?.path));
  let comparisonKey = $derived(`${effectiveBase}:${effectiveHead ?? "WORKTREE"}`);
  let terminal = $derived(getSettings().terminal);

  function cacheKey(file: ChangedFile): string {
    return `${comparisonKey}:${file.old_path ?? ""}:${file.path}`;
  }

  function fileName(path: string): string { return path.split("/").pop() || path; }
  function dirName(path: string): string { const parts = path.split("/"); return parts.length > 1 ? `${parts.slice(0, -1).join("/")}/` : ""; }
  function statusColor(status: string): string {
    return status === "A" ? "text-status-running" : status === "D" ? "text-status-exited" : status === "R" ? "text-accent" : "text-status-review";
  }

  function discardCommentsIfNeeded(message: string): boolean {
    if (getComments(sessionId).length === 0) return true;
    if (!window.confirm(message)) return false;
    clearComments(sessionId);
    return true;
  }

  async function refresh({ comparisonChange = false } = {}): Promise<void> {
    if (sendingFeedback || !discardCommentDraft()) return;
    if (comparisonChange && !discardCommentsIfNeeded("Discard pending review comments before changing the comparison?")) return;
    const generation = ++refreshGeneration;
    selectedLoadRequest++;
    inFlightDiffs = new Map();
    loading = true;
    selectedRange = null;
    cache = new Map();
    try {
      const nextFiles = await git.getChangedFiles(repoPath, effectiveBase, effectiveHead);
      if (generation !== refreshGeneration) return;
      const previousPath = activeFile?.path;
      const orphanedComments = getComments(sessionId).filter((comment) => !nextFiles.some((file) => file.path === comment.filePath));
      if (orphanedComments.length > 0 && !window.confirm(`Discard ${orphanedComments.length} comment${orphanedComments.length === 1 ? "" : "s"} for file${orphanedComments.length === 1 ? "" : "s"} no longer in this comparison?`)) return;
      orphanedComments.forEach((comment) => removeComment(sessionId, comment.id));
      const nextFolderPaths = getReviewFolderPaths(nextFiles);
      let nextExpandedFolders = reconcileReviewTreeExpansion(
        expandedSidebarFolders,
        sidebarFolderPaths,
        nextFolderPaths,
        sidebarTreeInitialized,
      );
      files = nextFiles;
      selectedIndex = Math.max(0, nextFiles.findIndex((file) => file.path === previousPath));
      if (selectedIndex < 0 || !nextFiles[selectedIndex]) selectedIndex = 0;
      sidebarFolderPaths = nextFolderPaths;
      sidebarTreeInitialized = true;
      if (nextFiles[selectedIndex]) {
        nextExpandedFolders = expandReviewTreeAncestors(nextExpandedFolders, nextFiles[selectedIndex].path);
        sidebarFocusedRowId = `file:${nextFiles[selectedIndex].path}`;
      } else {
        sidebarFocusedRowId = null;
      }
      expandedSidebarFolders = nextExpandedFolders;
      const fingerprints = new Map(nextFiles.map((file) => [file.path, `${file.status}:${file.additions}:${file.deletions}:${file.old_path ?? ""}`]));
      invalidateViewedFiles(sessionId, fingerprints);
      allFilesReviewed = false;
      if (nextFiles.length > 0) await loadSelected(generation);
      else activeDiff = null;
    } catch (error) {
      if (generation === refreshGeneration) {
        files = [];
        activeDiff = null;
        showSnackbar(`Could not load review files: ${String(error)}`, "error");
      }
    } finally {
      if (generation === refreshGeneration) loading = false;
    }
  }

  function fetchFileDiff(
    file: ChangedFile,
    key: string,
    cacheAtRequestStart: Map<string, ReviewFileDiff>,
    inFlightAtRequestStart: Map<string, Promise<ReviewFileDiff>>,
  ): Promise<ReviewFileDiff> {
    const existing = inFlightAtRequestStart.get(key);
    if (existing) return existing;

    const request = git.getFileDiff(repoPath, effectiveBase, file.path, file.old_path, effectiveHead) as Promise<ReviewFileDiff>;
    const cachedRequest = request.then((diff) => {
      if (cache === cacheAtRequestStart) cache.set(key, diff);
      return diff;
    }).finally(() => {
      if (inFlightDiffs === inFlightAtRequestStart && inFlightAtRequestStart.get(key) === cachedRequest) {
        inFlightAtRequestStart.delete(key);
      }
    });
    inFlightAtRequestStart.set(key, cachedRequest);
    return cachedRequest;
  }

  async function loadSelected(generation = refreshGeneration): Promise<void> {
    const file = files[selectedIndex];
    const requestId = ++selectedLoadRequest;
    if (!file) { activeDiff = null; fileLoading = false; return; }
    onFileChange?.(fileName(file.path));
    const key = cacheKey(file);
    const cached = cache.get(key);
    if (cached) { activeDiff = cached; fileLoading = false; return; }
    activeDiff = null;
    fileLoading = true;
    const cacheAtRequestStart = cache;
    const inFlightAtRequestStart = inFlightDiffs;
    try {
      const diff = await fetchFileDiff(file, key, cacheAtRequestStart, inFlightAtRequestStart);
      if (
        generation !== refreshGeneration
        || cache !== cacheAtRequestStart
        || requestId !== selectedLoadRequest
        || files[selectedIndex]?.path !== file.path
      ) return;
      activeDiff = diff;
      void preloadAdjacentFiles(generation, cacheAtRequestStart, inFlightAtRequestStart);
    } catch (error) {
      if (generation === refreshGeneration && requestId === selectedLoadRequest) {
        showSnackbar(`Could not load ${file.path}: ${String(error)}`, "error");
      }
    } finally {
      if (generation === refreshGeneration && requestId === selectedLoadRequest) fileLoading = false;
    }
  }

  async function preloadAdjacentFiles(
    generation: number,
    cacheAtRequestStart: Map<string, ReviewFileDiff>,
    inFlightAtRequestStart: Map<string, Promise<ReviewFileDiff>>,
  ): Promise<void> {
    const requests = [selectedIndex - 1, selectedIndex + 1].flatMap((index) => {
      const file = files[index];
      if (!file) return [];
      const key = cacheKey(file);
      return cacheAtRequestStart.has(key) ? [] : [{ file, key }];
    });
    await Promise.all(requests.map(async ({ file, key }) => {
      try {
        await fetchFileDiff(file, key, cacheAtRequestStart, inFlightAtRequestStart);
      } catch { /* Selected-file loading remains the only user-visible failure. */ }
    }));
  }

  function getSidebarRowId(row: ReviewTreeRow): string {
    return `${row.kind}:${row.path}`;
  }

  function revealSelectedFile(): void {
    const selectedPath = files[selectedIndex]?.path;
    if (!selectedPath) return;
    expandedSidebarFolders = expandReviewTreeAncestors(expandedSidebarFolders, selectedPath);
    sidebarFocusedRowId = `file:${selectedPath}`;
  }

  function selectFile(index: number): boolean {
    if (!files[index]) return false;
    if (index === selectedIndex) { revealSelectedFile(); return true; }
    if (showCommentInput && !discardCommentDraft()) return false;
    selectedIndex = index;
    selectedRange = null;
    diffFocus = "list";
    revealSelectedFile();
    void loadSelected();
    return true;
  }

  function discardCommentDraft(): boolean {
    if (!showCommentInput || !commentText.trim()) { cancelComment(); return true; }
    if (!window.confirm("Discard the unsaved review comment?")) return false;
    cancelComment();
    return true;
  }

  function setDiffStyle(style: "split" | "unified"): void {
    if (diffStyle === style || !discardCommentDraft()) return;
    diffStyle = style;
    selectedRange = null;
  }

  function toggleViewed(index = selectedIndex, advance = false): void {
    const file = files[index];
    if (!file) return;
    const viewed = isFileViewed(sessionId, file.path);
    if (viewed) setFileUnviewed(sessionId, file.path); else setFileViewed(sessionId, file.path);
    if (!advance || viewed) return;
    const next = files.findIndex((candidate, candidateIndex) => candidateIndex > index && !isFileViewed(sessionId, candidate.path));
    if (next >= 0) { selectFile(next); return; }
    const wrap = files.findIndex((candidate) => !isFileViewed(sessionId, candidate.path));
    if (wrap >= 0) { selectFile(wrap); return; }
    allFilesReviewed = true;
  }

  function openComment(selection = selectedRange): void {
    if (sendingFeedback || !selection || !activeTextDiff || !activeFile) return;
    if (showCommentInput && !discardCommentDraft()) return;
    commentRange = selection;
    commentType = selection.startLine === selection.endLine ? "line" : "hunk";
    commentText = "";
    editingCommentId = null;
    showCommentInput = true;
    requestAnimationFrame(() => commentInputEl?.focus());
  }

  function openEditComment(comment: ReviewComment): void {
    if (sendingFeedback) return;
    if (showCommentInput && editingCommentId === comment.id) {
      commentInputEl?.focus();
      return;
    }
    if (showCommentInput && !discardCommentDraft()) return;
    commentRange = { side: comment.side, startLine: comment.startLine, endLine: comment.endLine };
    commentType = comment.type;
    commentText = comment.text;
    editingCommentId = comment.id;
    showCommentInput = true;
    requestAnimationFrame(() => commentInputEl?.focus());
  }

  function cancelComment(): void {
    showCommentInput = false;
    commentText = "";
    commentRange = null;
    editingCommentId = null;
  }

  function submitComment(): void {
    const text = commentText.trim();
    if (sendingFeedback || !text || !activeFile || !activeTextDiff || !commentRange) return;
    if (editingCommentId) editComment(sessionId, editingCommentId, text);
    else addComment(sessionId, {
      filePath: activeFile.path,
      type: commentType,
      startLine: commentRange.startLine,
      endLine: commentRange.endLine,
      side: commentRange.side,
      comparisonKey,
      fingerprint: contentFingerprint(activeTextDiff),
      text,
    });
    cancelComment();
    selectedRange = null;
  }

  function handleCommentKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") { event.preventDefault(); cancelComment(); }
    else if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submitComment(); }
  }

  async function sendFeedback(): Promise<void> {
    const comments = getComments(sessionId);
    if (comments.length === 0 || sessionExited || sendingFeedback || showCommentInput) return;
    sendingFeedback = true;
    try {
      const diffs = new Map<string, TextFileDiff>();
      for (const filePath of new Set(comments.map((comment) => comment.filePath))) {
        const file = files.find((candidate) => candidate.path === filePath);
        if (!file) throw new Error(`${filePath} is no longer part of this comparison`);
        const latest = await git.getFileDiff(repoPath, effectiveBase, file.path, file.old_path, effectiveHead) as ReviewFileDiff;
        if (latest.kind !== "text") throw new Error(`${filePath} is binary and cannot receive line comments`);
        const expected = comments.filter((comment) => comment.filePath === filePath);
        if (expected.some((comment) => comment.comparisonKey !== comparisonKey || comment.fingerprint !== contentFingerprint(latest))) {
          throw new Error(`${filePath} changed since it was reviewed. Refresh and re-anchor its comments.`);
        }
        diffs.set(filePath, latest);
      }
      const bytes = Array.from(new TextEncoder().encode(serializeComments(comments, diffs)));
      recordUserInput(sessionId);
      await pty.write(sessionId, bytes);
      await pty.write(sessionId, [0x0d]);
      const count = comments.length;
      clearComments(sessionId);
      showSnackbar(`Feedback sent (${count} comment${count === 1 ? "" : "s"})`, "success");
    } catch (error) {
      showSnackbar(String(error).replace(/^Error: /, ""), "error");
    } finally {
      sendingFeedback = false;
    }
  }

  function editCommentAtSelection(): void {
    const selection = selectedRange;
    if (!selection || !activeFile) return;
    const comment = currentFileComments.find((candidate) =>
      candidate.side === selection.side
      && candidate.startLine <= selection.endLine
      && candidate.endLine >= selection.startLine,
    );
    if (comment) openEditComment(comment);
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (!visible || getActiveZone() !== "terminal") return;
    const element = document.activeElement;
    if (element?.closest("[role='dialog'], [role='alertdialog'], [role='combobox'], dialog[open]")) return;
    if (e.key === "Enter" && e.metaKey) { e.preventDefault(); void sendFeedback(); return; }
    if (sendingFeedback || showCommentInput || showCompareForm || allFilesReviewed) return;
    if (e.key === "?" || (e.key === "/" && e.shiftKey)) {
      e.preventDefault();
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", metaKey: true, bubbles: true }));
      return;
    }
    if ((e.key === "n" && e.ctrlKey) || (e.key === "ArrowDown" && e.ctrlKey)) {
      e.preventDefault();
      selectFile(Math.min(files.length - 1, selectedIndex + 1));
      return;
    }
    if ((e.key === "p" && e.ctrlKey) || (e.key === "ArrowUp" && e.ctrlKey)) {
      e.preventDefault();
      selectFile(Math.max(0, selectedIndex - 1));
      return;
    }
    if (e.key === "r" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); void refresh(); return; }
    if (e.key === "B" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); showCompareForm = !showCompareForm; return; }
    if (e.key === "u" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); setDiffStyle(diffStyle === "split" ? "unified" : "split"); return; }
    if (e.key === "m" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); toggleViewed(selectedIndex, true); return; }
    if (e.key === "e" && !e.metaKey && !e.ctrlKey && activeFile) { e.preventDefault(); onEditFile?.(activeFile.path); return; }
    if (e.key === "]" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); diffFocus = "body"; diffView?.nextHunk(); return; }
    if (e.key === "[" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); diffFocus = "body"; diffView?.previousHunk(); return; }
    if (e.key === "x" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); diffView?.expandContext(); return; }
    if (diffFocus === "list") {
      if (handleSidebarTreeKey(e)) return;
      if (e.key === "Escape") e.preventDefault();
      return;
    }
    if (e.key === "Escape") { e.preventDefault(); selectedRange = null; diffFocus = "list"; return; }
    if (e.key === "c" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      openComment();
    } else if (e.key === "E" && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      editCommentAtSelection();
    }
  }

  function toggleSidebarFolder(path: string): void {
    const expanded = new Set(expandedSidebarFolders);
    if (expanded.has(path)) expanded.delete(path);
    else expanded.add(path);
    expandedSidebarFolders = expanded;
    sidebarFocusedRowId = `folder:${path}`;
  }

  function focusSidebarRow(row: ReviewTreeRow): void {
    if (row.kind === "folder") sidebarFocusedRowId = getSidebarRowId(row);
    else if (!selectFile(row.fileIndex)) return;

    const rowId = getSidebarRowId(row);
    requestAnimationFrame(() => {
      Array.from(sidebarListRef?.querySelectorAll<HTMLElement>("[data-tree-row]") ?? []).find(
        (element) => element.dataset.treeRow === rowId,
      )?.focus({ preventScroll: true });
    });
  }

  function moveSidebarFocus(delta: 1 | -1): void {
    if (sidebarRows.length === 0) return;
    const focusedIndex = sidebarRows.findIndex((row) => getSidebarRowId(row) === sidebarFocusedRowId);
    const selectedRowIndex = sidebarRows.findIndex((row) => row.kind === "file" && row.fileIndex === selectedIndex);
    const currentIndex = focusedIndex >= 0 ? focusedIndex : Math.max(selectedRowIndex, 0);
    const nextIndex = Math.max(0, Math.min(sidebarRows.length - 1, currentIndex + delta));
    focusSidebarRow(sidebarRows[nextIndex]);
  }

  function parentFolderPath(row: ReviewTreeRow): string | null {
    const separator = row.path.lastIndexOf("/");
    return separator >= 0 ? row.path.slice(0, separator) : null;
  }

  function handleSidebarTreeKey(e: KeyboardEvent): boolean {
    const focusedRow = sidebarRows.find((row) => getSidebarRowId(row) === sidebarFocusedRowId)
      ?? sidebarRows.find((row) => row.kind === "file" && row.fileIndex === selectedIndex);
    if (!focusedRow) return false;

    const hasPlatformModifier = e.ctrlKey || e.metaKey;
    const isDown = !hasPlatformModifier && (e.key === "ArrowDown" || e.key === "j");
    const isUp = !hasPlatformModifier && (e.key === "ArrowUp" || e.key === "k");
    const isRight = !hasPlatformModifier && (e.key === "ArrowRight" || e.key === "l");
    const isLeft = !hasPlatformModifier && (e.key === "ArrowLeft" || e.key === "h");

    if (isDown) moveSidebarFocus(1);
    else if (isUp) moveSidebarFocus(-1);
    else if (isRight && focusedRow.kind === "folder") {
      if (!expandedSidebarFolders.has(focusedRow.path)) toggleSidebarFolder(focusedRow.path);
      else {
        const child = sidebarRows[sidebarRows.indexOf(focusedRow) + 1];
        if (child && child.depth > focusedRow.depth) focusSidebarRow(child);
      }
    } else if (isLeft) {
      if (focusedRow.kind === "folder" && expandedSidebarFolders.has(focusedRow.path)) {
        toggleSidebarFolder(focusedRow.path);
      } else {
        const parentPath = parentFolderPath(focusedRow);
        const parent = parentPath === null ? undefined : sidebarRows.find(
          (row) => row.kind === "folder" && row.path === parentPath,
        );
        if (parent) focusSidebarRow(parent);
      }
    } else if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) {
      if (focusedRow.kind === "folder") toggleSidebarFolder(focusedRow.path);
      else {
        diffFocus = "body";
        diffView?.focus();
      }
    } else return false;

    e.preventDefault();
    return true;
  }

  function handleSidebarRowKeydown(event: KeyboardEvent): void {
    if (!handleSidebarTreeKey(event)) return;
    event.stopPropagation();
  }

  onMount(() => {
    ensureSession(sessionId);
    window.addEventListener("keydown", handleKeydown);
    if (visible) { mounted = true; void refresh(); }
  });

  onDestroy(() => window.removeEventListener("keydown", handleKeydown));

  $effect(() => {
    if (visible && !mounted) { mounted = true; void refresh(); }
  });

  $effect(() => {
    const focusedRowId = sidebarFocusedRowId;
    void viewedVersion;
    if (!focusedRowId) return;
    Array.from(sidebarListRef?.querySelectorAll<HTMLElement>("[data-tree-row]") ?? []).find(
      (row) => row.dataset.treeRow === focusedRowId,
    )?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="flex h-full w-full" class:hidden={!visible}>
  <div class="relative min-w-0 flex-1 overflow-hidden">
    <div class="absolute left-0 right-0 top-0 z-10 flex h-[42px] items-center gap-3 border-b border-border bg-chrome px-4">
      {#if activeFile}
        <span class="truncate font-mono text-[12px] text-t1">{activeFile.path}</span>
        <span class="text-[11px] text-status-running">+{activeFile.additions}</span>
        <span class="text-[11px] text-status-exited">−{activeFile.deletions}</span>
      {/if}
      <div class="flex-1"></div>
      <button class="rounded-md px-2 py-1 font-mono text-[11px] text-t2 hover:bg-panel-hi hover:text-t1" onclick={() => { if (!sendingFeedback) showCompareForm = !showCompareForm; }} title="Change comparison (B)">{formatComparison(comparison)}</button>
      <div class="flex gap-0.5 rounded-lg bg-panel-hi p-0.5">
        <button class="rounded-md px-2 py-1 text-[11px] {diffStyle === 'unified' ? 'bg-accent-bg text-accent' : 'text-t2 hover:text-t1'}" onclick={() => setDiffStyle("unified")}>Unified</button>
        <button class="rounded-md px-2 py-1 text-[11px] {diffStyle === 'split' ? 'bg-accent-bg text-accent' : 'text-t2 hover:text-t1'}" onclick={() => setDiffStyle("split")}>Split</button>
      </div>
      {#if diffStyle === "unified"}
        <div class="flex gap-0.5 rounded-lg bg-panel-hi p-0.5" aria-label="Original-side review selection">
          <button
            type="button"
            class="rounded-md px-2 py-1 text-[11px] text-t2 hover:text-t1"
            onclick={() => diffView?.previousOriginalHunk()}
            title="Select previous original-side hunk (Option-Shift-Up)"
            aria-label="Select previous original-side hunk"
            aria-keyshortcuts="Alt+Shift+ArrowUp"
          >Original ↑</button>
          <button
            type="button"
            class="rounded-md px-2 py-1 text-[11px] text-t2 hover:text-t1"
            onclick={() => diffView?.nextOriginalHunk()}
            title="Select next original-side hunk (Option-Shift-Down)"
            aria-label="Select next original-side hunk"
            aria-keyshortcuts="Alt+Shift+ArrowDown"
          >Original ↓</button>
        </div>
      {/if}
      {#if selectedRange && activeTextDiff}
        <Button size="sm" onclick={() => openComment()}>Comment</Button>
      {/if}
      {#if totalCount > 0}
        <Button variant="primary" size="sm" onclick={sendFeedback} disabled={sessionExited || sendingFeedback || showCommentInput} title={sessionExited ? "Agent is not running" : showCommentInput ? "Submit or cancel the open comment before sending feedback" : `Send feedback (${MOD_ENTER_HINT})`}>
          {#if sendingFeedback}<LoaderCircle size={12} class="animate-spin" />{:else}<Send size={12} />{/if}<span class="ml-1">Send ({totalCount})</span>
        </Button>
      {/if}
    </div>

    {#if conflicted}
      <div class="absolute left-0 right-0 top-[42px] z-10 flex items-center gap-2 border-b border-[rgba(234,179,8,0.3)] bg-[rgba(234,179,8,0.12)] px-4 py-2"><AlertTriangle class="size-4 shrink-0 text-[#eab308]" /><span class="text-[12.5px] font-medium text-[#eab308]">This PR has merge conflicts</span></div>
    {/if}

    {#if showCompareForm}
      <BranchCompareForm {repoPath} {baseBranch} currentBase={effectiveBase} currentHead={effectiveHead} onConfirm={(baseRef, headRef) => {
        if (sendingFeedback || !discardCommentsIfNeeded("Discard pending review comments before changing the comparison?")) return;
        setComparison(sessionId, { baseRef, headRef });
        showCompareForm = false;
        void refresh({ comparisonChange: false });
      }} onCancel={() => (showCompareForm = false)} />
    {/if}

    {#if showCommentInput}
      <div class="absolute left-0 right-0 z-20 border-b border-border bg-chrome p-3" style:top="{contentTop}px">
        <div class="mb-1.5 text-[10px] text-t3">● {editingCommentId ? "Edit note" : "Review note"} · {commentRange?.side} {commentRange?.startLine === commentRange?.endLine ? `line ${commentRange?.startLine}` : `lines ${commentRange?.startLine}–${commentRange?.endLine}`}</div>
        <textarea bind:this={commentInputEl} bind:value={commentText} onkeydown={handleCommentKeydown} aria-label="Review comment" class="w-full resize-none rounded-lg border border-border-s bg-panel p-2 text-[12.5px] text-t1 focus:outline-none focus:ring-1 focus:ring-accent" rows="3" placeholder="Add a note… (Enter to submit, Esc to cancel)"></textarea>
      </div>
    {/if}

    {#if currentFileComments.length > 0 && !showCommentInput}
      <div class="absolute left-0 right-0 z-10 max-h-[160px] overflow-y-auto border-b border-border bg-chrome" style:top="{contentTop}px">
        {#each currentFileComments as comment (comment.id)}
          <div class="flex items-start gap-2 border-b border-border/50 px-4 py-2 last:border-b-0" role="group" aria-label="Review comment">
            <button type="button" class="min-w-0 flex-1 text-left" onclick={() => openEditComment(comment)}><span class="mr-2 font-mono text-[10px] text-t3">{comment.side} L{comment.startLine}{comment.startLine === comment.endLine ? "" : `–${comment.endLine}`}</span><span class="whitespace-pre-wrap text-[12px] text-t1">{comment.text}</span></button>
            <button class="text-[11px] text-t3 hover:text-t1" onclick={() => openEditComment(comment)} disabled={sendingFeedback} title="Edit" aria-label="Edit review comment">✎</button>
            <button class="text-[13px] text-t3 hover:text-t1" onclick={() => { if (!sendingFeedback) removeComment(sessionId, comment.id); }} disabled={sendingFeedback} title="Delete" aria-label="Delete review comment">×</button>
          </div>
        {/each}
      </div>
    {/if}

    <div class="absolute inset-0" style:top="{contentTop}px">
      {#if loading}
        <div class="flex h-full items-center justify-center bg-main text-t3">Loading diff…</div>
      {:else if files.length === 0}
        <div class="flex h-full items-center justify-center bg-main text-t3">No changes on this branch</div>
      {:else if allFilesReviewed}
        <div class="flex h-full flex-col items-center justify-center bg-main text-t3"><Check size={24} class="mb-2 text-status-running" /><span class="text-sm font-medium text-t2">All files reviewed</span></div>
      {:else if activeDiff?.kind === "binary"}
        <div class="flex h-full flex-col items-center justify-center gap-2 bg-main text-t3"><span class="text-sm font-medium text-t2">Binary file cannot be rendered as text</span><span class="font-mono text-xs">{activeDiff.original_size} B → {activeDiff.modified_size} B</span></div>
      {:else if activeTextDiff}
        <CodeMirrorDiff bind:this={diffView} diff={activeTextDiff} filePath={activeFile?.path ?? ""} mode={diffStyle} dark={isDark()} fontFamily={terminal.font_family} fontSize={terminal.font_size} comments={currentFileComments} onEditComment={openEditComment} onDeleteComment={(comment) => { if (!sendingFeedback) removeComment(sessionId, comment.id); }} onSelection={(selection) => { selectedRange = selection; diffFocus = "body"; }} />
      {:else if fileLoading}
        <div class="flex h-full items-center justify-center bg-main text-t3">Loading file…</div>
      {/if}
    </div>
  </div>

  <div class="relative shrink-0 overflow-x-hidden overflow-y-auto border-l border-border bg-chrome" style:width="{sidebarWidth}px">
    <ResizeHandle side="left" bind:width={sidebarWidth} min={180} max={Infinity} defaultWidth={240} onResizeEnd={(width) => setLayoutWidth("diff-sidebar", width)} />
    <div class="flex items-center justify-between border-b border-border px-3 py-2.5 text-[10px] font-semibold uppercase tracking-[.05em] text-t3"><span>Changed · {files.length}</span><span class="text-status-running">+{files.reduce((total, file) => total + file.additions, 0)}</span><span class="text-status-exited">−{files.reduce((total, file) => total + file.deletions, 0)}</span></div>
    <ul bind:this={sidebarListRef} class="py-1" role="tree" aria-label="Changed files">
      {#each sidebarRows as row (getSidebarRowId(row))}
        {@const rowId = getSidebarRowId(row)}
        {#if row.kind === "folder"}
          {@const expanded = expandedSidebarFolders.has(row.path)}
          <li
            role="treeitem"
            aria-level={row.depth + 1}
            aria-expanded={expanded}
            aria-selected={false}
            tabindex={sidebarFocusedRowId === rowId ? 0 : -1}
            data-tree-row={rowId}
            class="mx-1 flex w-full items-center gap-1.5 rounded-lg py-1.5 pr-2 text-left text-[12px] font-mono outline-none focus-visible:ring-2 focus-visible:ring-accent {sidebarFocusedRowId === rowId ? 'bg-panel-hi text-t1' : 'text-t2 hover:bg-panel-hi'}"
            style={`padding-left: ${8 + row.depth * 12}px`}
            onfocus={() => (sidebarFocusedRowId = rowId)}
            onkeydown={handleSidebarRowKeydown}
            onclick={() => toggleSidebarFolder(row.path)}
          >
            {#if expanded}<ChevronDown size={13} class="shrink-0" />{:else}<ChevronRight size={13} class="shrink-0" />{/if}
            <span class="truncate" title={row.path}>{row.name}</span>
          </li>
        {:else}
          {@const file = files[row.fileIndex]}
          {@const count = getFileCommentCount(sessionId, file.path)}
          {@const viewed = viewedFiles.has(file.path)}
          <li
            role="treeitem"
            aria-level={row.depth + 1}
            aria-selected={row.fileIndex === selectedIndex}
            tabindex={sidebarFocusedRowId === rowId ? 0 : -1}
            data-tree-row={rowId}
            class="mx-1 flex items-center gap-1 outline-none focus-visible:ring-2 focus-visible:ring-accent {viewed ? 'opacity-50' : ''}"
            onfocus={() => (sidebarFocusedRowId = rowId)}
            onkeydown={handleSidebarRowKeydown}
            onclick={() => selectFile(row.fileIndex)}
          >
            <span class="w-0.5 self-stretch rounded-full {row.fileIndex === selectedIndex ? 'bg-accent' : 'opacity-0'}"></span>
            <div
              class="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-lg py-1.5 pr-2 text-[12px] {row.fileIndex === selectedIndex ? 'bg-accent-bg' : sidebarFocusedRowId === rowId ? 'bg-panel-hi' : 'hover:bg-panel-hi'}"
              style={`padding-left: ${8 + row.depth * 12}px`}
            >
              {#if viewed}<button tabindex={-1} class="shrink-0 text-status-running" onclick={(event) => { event.stopPropagation(); toggleViewed(row.fileIndex); }} title="Mark as unviewed" aria-label="Mark as unviewed"><Check size={12} /></button>{:else}<span class="w-4 shrink-0 font-mono text-[10px] {statusColor(file.status)}">{file.status}</span>{/if}
              <span class="min-w-0 flex-1 truncate font-mono text-[12px] text-t1" title={file.path}>{row.name}</span>
              {#if count > 0}<span class="flex items-center gap-0.5 text-[10px] text-accent"><MessageSquare size={10} />{count}</span>{/if}
              <span class="font-mono text-[10px] text-t3">+{file.additions} −{file.deletions}</span>
            </div>
          </li>
        {/if}
      {/each}
    </ul>
  </div>
</div>
