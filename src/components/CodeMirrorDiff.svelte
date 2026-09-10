<script lang="ts">
  import { onDestroy } from "svelte";
  import { EditorState, StateEffect, StateField, Text, type Extension } from "@codemirror/state";
  import { Decoration, EditorView, WidgetType, keymap, type DecorationSet } from "@codemirror/view";
  import {
    cursorDocEnd,
    cursorDocStart,
    cursorLineDown,
    cursorLineUp,
    cursorPageDown,
    cursorPageUp,
  } from "@codemirror/commands";
  import {
    Chunk,
    MergeView,
    getChunks,
    getOriginalDoc,
    goToNextChunk,
    goToPreviousChunk,
    unifiedMergeView,
    uncollapseUnchanged,
  } from "@codemirror/merge";
  import { basicSetup } from "codemirror";
  import { darkTheme, detectLanguageFromPath, fontExtension, lightTheme } from "../lib/cm-shared";
  import type { ReviewSelection, TextFileDiff } from "../lib/review-diff";
  import {
    documentRangeForVisualLines,
    reviewSelectionForDocumentRange,
  } from "../lib/cm-review-selection";
  import type { ReviewComment } from "../lib/review-comments.svelte";
  import { renderCommentAnnotation } from "../lib/comment-annotation";
  import {
    combineOriginalSelections,
    originalSelectionForChunk,
    originalSelectionForDeletedLine,
  } from "../lib/cm-unified-selection";

  interface Props {
    diff: TextFileDiff | null;
    filePath: string;
    mode: "split" | "unified";
    dark: boolean;
    fontFamily: string;
    fontSize: number;
    onSelection?: (selection: ReviewSelection | null) => void;
    comments?: readonly ReviewComment[];
    onEditComment?: (comment: ReviewComment) => void;
    onDeleteComment?: (comment: ReviewComment) => void;
  }

  let { diff, filePath, mode, dark, fontFamily, fontSize, onSelection, comments = [], onEditComment, onDeleteComment }: Props = $props();
  let root = $state<HTMLElement>();
  let merge: MergeView | null = null;
  let unified: EditorView | null = null;
  let languageExtension = $state<Extension[]>([]);
  let deletedSelectionAnchor: ReviewSelection | null = null;
  let visualSelections = new WeakMap<EditorView, { anchorLine: number; headLine: number; headPosition: number }>();
  let visualSelectionUpdates = new WeakSet<EditorView>();
  let selectedOriginalChunkIndex: number | null = null;
  let generation = 0;

  const collapseUnchanged = { margin: 3, minSize: 4 };
  const diffConfig = { scanLimit: 500, timeout: 100 };

  class ReviewCommentWidget extends WidgetType {
    readonly comment: ReviewComment;

    constructor(comment: ReviewComment) {
      super();
      this.comment = comment;
    }

    eq(other: ReviewCommentWidget): boolean {
      return other.comment === this.comment;
    }

    toDOM(): HTMLElement {
      return renderCommentAnnotation(this.comment, {
        onOpen: () => onEditComment?.(this.comment),
        onEdit: () => onEditComment?.(this.comment),
        onDelete: () => onDeleteComment?.(this.comment),
        onContextMenu: (event) => event.preventDefault(),
      });
    }
  }

  function textForDiff(content: string): Text {
    return Text.of(content.replace(/\r\n?/g, "\n").split("\n"));
  }

  const setCommentDecorations = StateEffect.define<DecorationSet>();

  function commentDecorations(side: "original" | "modified" | "unified", document: Text, original?: Text): DecorationSet {
    const chunks = side === "unified" && original ? Chunk.build(original, document, diffConfig) : [];
    const decorations = comments.flatMap((comment) => {
      if (comment.type === "file") return [];
      if (side !== "unified" && comment.side !== side) return [];

      let position: number;
      let widgetSide = 1;
      if (side === "unified" && comment.side === "original") {
        if (!original || comment.endLine > original.lines) return [];
        const originalPosition = original.line(comment.endLine).to;
        const chunk = chunks.find((candidate) => candidate.fromA <= originalPosition && candidate.endA >= originalPosition);
        if (!chunk) return [];
        position = chunk.fromB;
        widgetSide = -1;
      } else {
        if (comment.endLine > document.lines) return [];
        position = document.line(comment.endLine).to;
      }
      return [Decoration.widget({ widget: new ReviewCommentWidget(comment), block: true, side: widgetSide }).range(position)];
    });
    return Decoration.set(decorations, true);
  }

  function commentExtensions(side: "original" | "modified" | "unified", document: Text, original?: Text): Extension[] {
    const decorations = StateField.define<DecorationSet>({
      create: () => commentDecorations(side, document, original),
      update: (value, transaction) => {
        const update = transaction.effects.find((effect) => effect.is(setCommentDecorations));
        return update ? update.value : value.map(transaction.changes);
      },
      provide: (field) => EditorView.decorations.from(field),
    });
    return [decorations];
  }

  function refreshCommentDecorations(): void {
    if (!diff) return;
    const original = textForDiff(diff.original);
    const modified = textForDiff(diff.modified);
    if (mode === "split" && merge) {
      merge.a.dispatch({ effects: setCommentDecorations.of(commentDecorations("original", original)) });
      merge.b.dispatch({ effects: setCommentDecorations.of(commentDecorations("modified", modified)) });
    } else if (mode === "unified" && unified) {
      unified.dispatch({ effects: setCommentDecorations.of(commentDecorations("unified", modified, original)) });
    }
  }

  function selectionFor(view: EditorView, side: ReviewSelection["side"]): ReviewSelection {
    const selection = view.state.selection.main;
    return reviewSelectionForDocumentRange(view.state.doc, side, selection.from, selection.to);
  }

  function unifiedDeletedSelection(event: Event, view: EditorView): ReviewSelection | null {
    const target = event.target;
    if (!(target instanceof Element)) return null;
    const deletedLine = target.closest(".cm-deletedLine");
    const deletedChunk = deletedLine?.closest(".cm-deletedChunk");
    if (!deletedLine || !deletedChunk) return null;

    const deletedLineOffset = Array.from(deletedChunk.querySelectorAll(".cm-deletedLine")).indexOf(deletedLine);
    const anchor = view.posAtDOM(deletedChunk);
    const chunk = getChunks(view.state)?.chunks.find((candidate) => candidate.fromB <= anchor && anchor <= candidate.endB);
    return chunk ? originalSelectionForDeletedLine(getOriginalDoc(view.state), chunk, deletedLineOffset) : null;
  }

  function selectOriginalChunk(view: EditorView, direction: 1 | -1): boolean {
    const original = getOriginalDoc(view.state);
    const chunks = getChunks(view.state)?.chunks.filter((chunk) => chunk.fromA < chunk.toA) ?? [];
    if (!original || chunks.length === 0) return false;

    const position = view.state.selection.main.head;
    const initialIndex = direction === 1
      ? chunks.findIndex((chunk) => chunk.fromB >= position)
      : chunks.findLastIndex((chunk) => chunk.fromB <= position);
    const index = selectedOriginalChunkIndex === null
      ? initialIndex >= 0 ? initialIndex : direction === 1 ? 0 : chunks.length - 1
      : (selectedOriginalChunkIndex + direction + chunks.length) % chunks.length;
    const chunk = chunks[index];
    const selection = originalSelectionForChunk(original, chunk);
    if (!selection) return false;

    selectedOriginalChunkIndex = index;
    visualSelections.delete(view);
    view.dispatch({ selection: { anchor: Math.min(chunk.fromB, view.state.doc.length) }, scrollIntoView: true });
    onSelection?.(selection);
    return true;
  }

  function setVisualSelection(
    view: EditorView,
    anchorLine: number,
    headLine: number,
    headPosition = view.state.doc.line(headLine).from,
  ): void {
    visualSelections.set(view, { anchorLine, headLine, headPosition });
    visualSelectionUpdates.add(view);
    view.dispatch({ selection: documentRangeForVisualLines(view.state.doc, anchorLine, headLine) });
  }

  function toggleVisualSelection(view: EditorView): boolean {
    const current = visualSelections.get(view);
    if (current) {
      visualSelections.delete(view);
      view.dispatch({ selection: { anchor: view.state.selection.main.head } });
      return true;
    }

    const headPosition = view.state.selection.main.head;
    const line = view.state.doc.lineAt(headPosition).number;
    setVisualSelection(view, line, line, headPosition);
    return true;
  }

  function moveReviewSelection(view: EditorView, move: (view: EditorView) => boolean): boolean {
    const current = visualSelections.get(view);
    if (!current) return move(view);

    // Move a collapsed cursor from the precise visual head, then rebuild the
    // linewise range from CodeMirror's actual result. Retaining the position
    // (rather than just its logical line) lets wrapped-line movement progress.
    visualSelectionUpdates.add(view);
    view.dispatch({ selection: { anchor: current.headPosition } });
    move(view);
    const headPosition = view.state.selection.main.head;
    const headLine = view.state.doc.lineAt(headPosition).number;
    setVisualSelection(view, current.anchorLine, headLine, headPosition);
    // Consume the key even at a document boundary; otherwise a lower-precedence
    // binding can collapse the rebuilt visual selection.
    return true;
  }

  function moveReviewSelectionByPage(view: EditorView, direction: 1 | -1): boolean {
    const current = visualSelections.get(view);
    if (!current) return direction === 1 ? cursorPageDown(view) : cursorPageUp(view);

    const pageHeight = root?.clientHeight ?? view.dom.clientHeight;
    const headCoords = view.coordsAtPos(current.headPosition);
    if (!headCoords || pageHeight <= 0) {
      return moveReviewSelection(view, direction === 1 ? cursorPageDown : cursorPageUp);
    }

    // The review container owns scrolling, so derive the target from its actual
    // viewport height instead of CodeMirror's non-scrolling editor viewport.
    const target = view.lineBlockAtHeight(
      Math.max(0, headCoords.top - view.documentTop + direction * pageHeight),
    );
    const headPosition = target.from;
    root?.scrollBy({ top: direction * pageHeight });
    setVisualSelection(
      view,
      current.anchorLine,
      view.state.doc.lineAt(headPosition).number,
      headPosition,
    );
    return true;
  }

  const reviewKeymap = keymap.of([
    { key: "j", run: (view) => moveReviewSelection(view, cursorLineDown) },
    { key: "ArrowDown", run: (view) => moveReviewSelection(view, cursorLineDown) },
    { key: "k", run: (view) => moveReviewSelection(view, cursorLineUp) },
    { key: "ArrowUp", run: (view) => moveReviewSelection(view, cursorLineUp) },
    { key: "v", run: toggleVisualSelection },
    { key: "d", run: (view) => moveReviewSelectionByPage(view, 1) },
    { key: "f", run: (view) => moveReviewSelectionByPage(view, 1) },
    { key: "PageDown", run: (view) => moveReviewSelectionByPage(view, 1) },
    { key: "b", run: (view) => moveReviewSelectionByPage(view, -1) },
    { key: "PageUp", run: (view) => moveReviewSelectionByPage(view, -1) },
    { key: "g", run: (view) => moveReviewSelection(view, cursorDocStart) },
    { key: "Home", run: (view) => moveReviewSelection(view, cursorDocStart) },
    { key: "G", run: (view) => moveReviewSelection(view, cursorDocEnd) },
    { key: "End", run: (view) => moveReviewSelection(view, cursorDocEnd) },
    { key: "Alt-Shift-ArrowUp", run: (view) => selectOriginalChunk(view, -1) },
    { key: "Alt-Shift-ArrowDown", run: (view) => selectOriginalChunk(view, 1) },
  ]);

  function extensions(side: ReviewSelection["side"], editorLabel: string) {
    return [
      reviewKeymap,
      basicSetup,
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.lineWrapping,
      EditorView.contentAttributes.of({ "aria-label": editorLabel }),
      dark ? darkTheme : lightTheme,
      fontExtension(fontFamily, fontSize),
      ...languageExtension,
      EditorView.updateListener.of((update) => {
        if (!update.selectionSet) return;
        if (!visualSelectionUpdates.delete(update.view)) visualSelections.delete(update.view);
        onSelection?.(selectionFor(update.view, side));
      }),
      EditorView.domEventHandlers({
        focus: (_, view) => onSelection?.(selectionFor(view, side)),
        pointerdown: (event, view) => {
          deletedSelectionAnchor = unifiedDeletedSelection(event, view);
        },
        pointerup: (event, view) => {
          const selection = unifiedDeletedSelection(event, view);
          const range = selection && deletedSelectionAnchor
            ? combineOriginalSelections(deletedSelectionAnchor, selection)
            : null;
          deletedSelectionAnchor = null;
          onSelection?.(range ?? selection ?? selectionFor(view, side));
        },
      }),
    ];
  }

  function destroy(): void {
    merge?.destroy();
    merge = null;
    unified?.destroy();
    unified = null;
    root?.replaceChildren();
  }

  function mount(): void {
    selectedOriginalChunkIndex = null;
    destroy();
    if (!root || !diff) return;
    if (mode === "split") {
      const original = textForDiff(diff.original);
      const modified = textForDiff(diff.modified);
      merge = new MergeView({
        parent: root,
        a: { doc: diff.original, extensions: [...extensions("original", "Original version"), ...commentExtensions("original", original)] },
        b: { doc: diff.modified, extensions: [...extensions("modified", "Modified version"), ...commentExtensions("modified", modified)] },
        orientation: "a-b",
        gutter: true,
        highlightChanges: false,
        collapseUnchanged,
        diffConfig,
      });
      return;
    }
    const original = textForDiff(diff.original);
    const modified = textForDiff(diff.modified);
    unified = new EditorView({
      parent: root,
      doc: diff.modified,
      extensions: [
        ...extensions("modified", "Unified diff"),
        ...commentExtensions("unified", modified, original),
        unifiedMergeView({
          original: diff.original,
          gutter: true,
          highlightChanges: false,
          mergeControls: false,
          collapseUnchanged,
          diffConfig,
        }),
      ],
    });
  }

  function loadLanguage(): void {
    const currentGeneration = ++generation;
    const language = detectLanguageFromPath(filePath);
    if (!language) {
      languageExtension = [];
      mount();
      return;
    }
    language.load().then((support) => {
      if (currentGeneration !== generation) return;
      languageExtension = [support.extension];
      mount();
    });
  }

  $effect(() => {
    void diff;
    void filePath;
    void mode;
    void dark;
    void fontFamily;
    void fontSize;
    if (!root) return;
    loadLanguage();
  });

  $effect(() => {
    void comments;
    refreshCommentDecorations();
  });

  export function focus(): void {
    (mode === "split" ? merge?.b : unified)?.focus();
  }

  export function nextHunk(): void {
    const view = mode === "split" ? merge?.b : unified;
    if (view) goToNextChunk(view);
  }

  export function previousHunk(): void {
    const view = mode === "split" ? merge?.b : unified;
    if (view) goToPreviousChunk(view);
  }

  export function previousOriginalHunk(): void {
    if (unified) selectOriginalChunk(unified, -1);
  }

  export function nextOriginalHunk(): void {
    if (unified) selectOriginalChunk(unified, 1);
  }

  export function expandContext(): void {
    const view = mode === "split" ? merge?.b : unified;
    if (!view) return;
    view.dispatch({ effects: uncollapseUnchanged.of(view.state.selection.main.head) });
  }

  onDestroy(destroy);
</script>

<div bind:this={root} class="cm-review-diff h-full overflow-auto" aria-label="Code review diff"></div>

<style>
  :global(.cm-review-diff .cm-mergeView) {
    min-height: 100%;
  }
  :global(.cm-review-diff .cm-editor) {
    height: auto;
  }
  :global(.cm-review-diff .cm-scroller) {
    overflow: visible;
  }
</style>
