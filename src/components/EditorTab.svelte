<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, onDestroy } from "svelte";
  import { EditorView, keymap } from "@codemirror/view";
  import { EditorState } from "@codemirror/state";
  import { vim, Vim, getCM } from "@replit/codemirror-vim";
  import { history, historyKeymap } from "@codemirror/commands";
  import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
  import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import {
    fontCompartment,
    themeCompartment,
    langCompartment,
    baseExtensions,
    darkTheme,
    lightTheme,
    fontExtension,
    isDarkTheme,
    detectLanguageFromPath,
  } from "../lib/cm-shared";
  import { getSettings } from "../lib/settings.svelte";

  interface Buffer {
    path: string;
    content: string;
    modified: boolean;
    state: EditorState | null;
  }

  interface Props {
    repoPath: string;
    visible: boolean;
    theme?: string;
    onClose: () => void;
    onFocusEditor: () => void;
  }

  let { repoPath, visible, theme = "vs-dark", onClose, onFocusEditor }: Props = $props();

  let buffers = $state<Buffer[]>([]);
  let activeIndex = $state(-1);
  let vimMode = $state("NORMAL");
  let cursorLine = $state(1);
  let cursorCol = $state(1);
  let editorContainer: HTMLElement;
  let view: EditorView | null = null;
  let mounted = false;

  const activeBuffer = $derived(activeIndex >= 0 ? buffers[activeIndex] : null);

  function createEditorState(content: string, filePath: string): EditorState {
    const themeExt = isDarkTheme(theme) ? darkTheme : lightTheme;
    const { font_family, font_size } = getSettings().terminal;

    return EditorState.create({
      doc: content,
      extensions: [
        vim(),
        baseExtensions,
        history(),
        closeBrackets(),
        highlightSelectionMatches(),
        keymap.of([...closeBracketsKeymap, ...historyKeymap, ...searchKeymap]),
        themeCompartment.of(themeExt),
        fontCompartment.of(fontExtension(font_family, font_size)),
        langCompartment.of([]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && activeBuffer) {
            activeBuffer.modified = true;
          }
          if (update.selectionSet || update.docChanged) {
            const pos = update.state.selection.main.head;
            const line = update.state.doc.lineAt(pos);
            cursorLine = line.number;
            cursorCol = pos - line.from + 1;
          }
        }),
      ],
    });
  }

  function setupView() {
    if (!editorContainer || view) return;
    view = new EditorView({ parent: editorContainer });

    // Register custom ex commands
    Vim.defineEx("w", "w", () => saveCurrentBuffer());
    Vim.defineEx("q", "q", (cm: any, params: any) => {
      if (params?.bang) {
        closeCurrentBuffer(true);
      } else {
        closeCurrentBuffer(false);
      }
    });
    Vim.defineEx("qa", "qa", () => onClose());
    Vim.defineEx("wq", "wq", async () => {
      await saveCurrentBuffer();
      closeCurrentBuffer(true);
    });
    Vim.defineEx("bn", "bn", () => nextBuffer());
    Vim.defineEx("bp", "bp", () => prevBuffer());

    // Track vim mode changes
    const interval = setInterval(() => {
      if (!view) return;
      const cm = getCM(view);
      if (cm) {
        const state = (cm as any).state;
        const mode = state?.vim?.mode || "normal";
        const sub = state?.vim?.subMode;
        if (mode === "insert") vimMode = "INSERT";
        else if (mode === "visual") vimMode = sub === "linewise" ? "V-LINE" : "VISUAL";
        else if (mode === "replace") vimMode = "REPLACE";
        else vimMode = "NORMAL";
      }
    }, 50);

    return () => clearInterval(interval);
  }

  let cleanupInterval: (() => void) | undefined;

  export async function openFile(filePath: string) {
    // Check if already open
    const existingIdx = buffers.findIndex((b) => b.path === filePath);
    if (existingIdx >= 0) {
      switchToBuffer(existingIdx);
      return;
    }

    const fullPath = `${repoPath}/${filePath}`;
    try {
      const content = await invoke<string>("read_file", { filePath: fullPath });
      const state = createEditorState(content, filePath);
      buffers.push({ path: filePath, content, modified: false, state });
      switchToBuffer(buffers.length - 1);
    } catch (e) {
      console.error("Failed to open file:", e);
    }
  }

  function switchToBuffer(index: number) {
    if (index < 0 || index >= buffers.length) return;

    // Save current buffer state
    if (activeIndex >= 0 && activeIndex < buffers.length && view) {
      buffers[activeIndex].state = view.state;
    }

    activeIndex = index;
    const buf = buffers[index];

    if (view && buf.state) {
      view.setState(buf.state);
      applyLanguage(buf.path);
    }
  }

  function applyLanguage(filePath: string) {
    const langDesc = detectLanguageFromPath(filePath);
    if (!langDesc) {
      view?.dispatch({ effects: langCompartment.reconfigure([]) });
      return;
    }
    langDesc.load().then((support) => {
      if (!view || buffers[activeIndex]?.path !== filePath) return;
      view.dispatch({ effects: langCompartment.reconfigure(support.extension) });
    });
  }

  async function saveCurrentBuffer() {
    if (!activeBuffer || !view) return;
    const content = view.state.doc.toString();
    const fullPath = `${repoPath}/${activeBuffer.path}`;
    try {
      await invoke("write_file", { filePath: fullPath, content });
      activeBuffer.modified = false;
      activeBuffer.content = content;
    } catch (e) {
      console.error("Failed to save file:", e);
    }
  }

  function closeCurrentBuffer(force: boolean) {
    if (!activeBuffer) return;
    if (activeBuffer.modified && !force) {
      console.warn("Buffer has unsaved changes. Use :q! to force.");
      return;
    }
    buffers.splice(activeIndex, 1);
    if (buffers.length === 0) {
      activeIndex = -1;
      onClose();
    } else {
      switchToBuffer(Math.min(activeIndex, buffers.length - 1));
    }
  }

  function nextBuffer() {
    if (buffers.length <= 1) return;
    switchToBuffer((activeIndex + 1) % buffers.length);
  }

  function prevBuffer() {
    if (buffers.length <= 1) return;
    switchToBuffer((activeIndex - 1 + buffers.length) % buffers.length);
  }

  export function focus() {
    view?.focus();
  }

  export async function save() {
    await saveCurrentBuffer();
  }

  export function closeBuffer() {
    closeCurrentBuffer(false);
  }

  onMount(() => {
    // Lazy setup on first visibility
  });

  onDestroy(() => {
    cleanupInterval?.();
    view?.destroy();
    view = null;
  });

  $effect(() => {
    if (visible && !mounted && editorContainer) {
      mounted = true;
      cleanupInterval = setupView();
    }
  });

  $effect(() => {
    if (visible && view) {
      view.focus();
    }
  });

  $effect(() => {
    const themeExt = isDarkTheme(theme) ? darkTheme : lightTheme;
    view?.dispatch({ effects: themeCompartment.reconfigure(themeExt) });
  });

  $effect(() => {
    const { font_family, font_size } = getSettings().terminal;
    view?.dispatch({ effects: fontCompartment.reconfigure(fontExtension(font_family, font_size)) });
  });

  function fileName(path: string): string {
    return path.split("/").pop() || path;
  }
</script>

<div class="flex flex-col h-full w-full" class:hidden={!visible}>
  <!-- Editor area -->
  <div bind:this={editorContainer} class="flex-1 min-w-0 relative overflow-hidden"></div>

  {#if !activeBuffer}
    <div class="absolute inset-0 flex items-center justify-center text-surface-500 bg-surface-50 dark:bg-surface-900">
      <span class="text-sm">Press <kbd class="px-1.5 py-0.5 rounded bg-surface-200 dark:bg-surface-700 text-xs font-mono">⌘P</kbd> to open a file</span>
    </div>
  {/if}

  <!-- Status bar -->
  {#if activeBuffer}
    <div class="flex items-center justify-between px-3 py-0.5 text-xs font-mono border-t border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-900 text-surface-600 dark:text-surface-400">
      <div class="flex items-center gap-3">
        <span class="font-bold {vimMode === 'INSERT' ? 'text-green-500' : vimMode === 'VISUAL' || vimMode === 'V-LINE' ? 'text-purple-400' : 'text-blue-400'}">
          {vimMode}
        </span>
        <span>
          {activeBuffer.path}{#if activeBuffer.modified}<span class="text-yellow-400 ml-1">●</span>{/if}
        </span>
      </div>
      <div class="flex items-center gap-3">
        {#if buffers.length > 1}
          <span class="text-surface-500">[{activeIndex + 1}/{buffers.length}]</span>
        {/if}
        <span>{cursorLine}:{cursorCol}</span>
      </div>
    </div>
  {/if}
</div>
