<script lang="ts">
  import { onDestroy } from "svelte";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import {
    MergeView,
    goToNextChunk,
    goToPreviousChunk,
    unifiedMergeView,
    uncollapseUnchanged,
  } from "@codemirror/merge";
  import { basicSetup } from "codemirror";
  import { darkTheme, detectLanguageFromPath, fontExtension, lightTheme } from "../lib/cm-shared";
  import type { ReviewSelection, TextFileDiff } from "../lib/review-diff";

  interface Props {
    diff: TextFileDiff | null;
    filePath: string;
    mode: "split" | "unified";
    dark: boolean;
    fontFamily: string;
    fontSize: number;
    onSelection?: (selection: ReviewSelection | null) => void;
  }

  let { diff, filePath, mode, dark, fontFamily, fontSize, onSelection }: Props = $props();
  let root = $state<HTMLElement>();
  let merge: MergeView | null = null;
  let unified: EditorView | null = null;
  let languageExtension = $state<Extension[]>([]);
  let generation = 0;

  const collapseUnchanged = { margin: 3, minSize: 4 };
  const diffConfig = { scanLimit: 500, timeout: 100 };

  function selectionFor(view: EditorView, side: ReviewSelection["side"]): ReviewSelection | null {
    const selection = view.state.selection.main;
    const start = view.state.doc.lineAt(selection.from).number;
    const end = view.state.doc.lineAt(selection.to).number;
    return { side, startLine: Math.min(start, end), endLine: Math.max(start, end) };
  }

  function extensions(side: ReviewSelection["side"]) {
    return [
      basicSetup,
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      dark ? darkTheme : lightTheme,
      fontExtension(fontFamily, fontSize),
      ...languageExtension,
      EditorView.updateListener.of((update) => {
        if (update.selectionSet) onSelection?.(selectionFor(update.view, side));
      }),
      EditorView.domEventHandlers({
        focus: (_, view) => onSelection?.(selectionFor(view, side)),
        pointerup: (_, view) => onSelection?.(selectionFor(view, side)),
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
    destroy();
    if (!root || !diff) return;
    if (mode === "split") {
      merge = new MergeView({
        parent: root,
        a: { doc: diff.original, extensions: extensions("original") },
        b: { doc: diff.modified, extensions: extensions("modified") },
        orientation: "a-b",
        gutter: true,
        highlightChanges: false,
        collapseUnchanged,
        diffConfig,
      });
      return;
    }
    unified = new EditorView({
      parent: root,
      doc: diff.modified,
      extensions: [
        ...extensions("modified"),
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
