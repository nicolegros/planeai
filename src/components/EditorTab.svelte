<script lang="ts">
  import { git } from "../lib/api";
  import { onMount, onDestroy } from "svelte";
  import { EditorView, keymap } from "@codemirror/view";
  import { EditorState, Compartment, Prec } from "@codemirror/state";
  import { vim } from "@replit/codemirror-vim";
  import { registerEditor, unregisterEditor } from "../lib/vim-registry";
  import { basicSetup } from "codemirror";
  import { defaultKeymap } from "@codemirror/commands";
  import { searchKeymap } from "@codemirror/search";
  import {
    darkTheme,
    lightTheme,
    fontExtension,
    isDarkTheme,
    detectLanguageFromPath,
  } from "../lib/cm-shared";
  import { syntaxHighlighting } from "@codemirror/language";
  import { classHighlighter } from "@lezer/highlight";
  import { getSettings } from "../lib/settings.svelte";
  import { MOD_LABEL } from "../lib/keyboard";

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
    initialFile?: string;
    onClose: () => void;
    onFocusEditor: () => void;
    onFileChange?: (fileName: string) => void;
    onModifiedChange?: (modified: boolean) => void;
  }

  let { repoPath, visible, theme = "vs-dark", initialFile, onClose, onFocusEditor, onFileChange, onModifiedChange }: Props = $props();

  let buffers = $state<Buffer[]>([]);
  let activeIndex = $state(-1);
  let vimMode = $state("NORMAL");
  let cursorLine = $state(1);
  let cursorCol = $state(1);
  let editorContainer: HTMLElement;
  let view: EditorView | null = null;
  let mounted = false;
  let initialFileOpened = false;

  // Auto-open initialFile when provided
  $effect(() => {
    if (initialFile && mounted && !initialFileOpened) {
      initialFileOpened = true;
      openFile(initialFile);
    }
  });

  // Editor-local compartments (not shared with diff renderer)
  const editorFontCompartment = new Compartment();
  const editorThemeCompartment = new Compartment();
  const editorLangCompartment = new Compartment();

  const activeBuffer = $derived(activeIndex >= 0 ? buffers[activeIndex] : null);

  function createEditorState(content: string, filePath: string): EditorState {
    const themeExt = isDarkTheme(theme) ? darkTheme : lightTheme;
    const { font_family, font_size } = getSettings().terminal;
    const useVim = getSettings().vim_mode ?? true;

    return EditorState.create({
      doc: content,
      extensions: [
        Prec.highest(keymap.of([
          { key: "Mod-t", run: () => false },
          { key: "Mod-w", run: () => false },
          { key: "Mod-d", run: () => false },
          { key: "Mod-Shift-[", run: () => false },
          { key: "Mod-Shift-]", run: () => false },
          { key: "Mod-b", run: () => false },
          { key: "Mod-k", run: () => false },
          { key: "Mod-e", run: () => false },
          { key: "Mod-,", run: () => false },
        ])),
        ...(useVim ? [vim()] : []),
        basicSetup,
        syntaxHighlighting(classHighlighter),
        ...(useVim ? [] : [keymap.of(defaultKeymap)]),
        editorThemeCompartment.of(themeExt),
        editorFontCompartment.of(fontExtension(font_family, font_size)),
        editorLangCompartment.of([]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && activeBuffer) {
            if (!activeBuffer.modified) onModifiedChange?.(true);
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
    if (!editorContainer) return;
    return () => {};
  }

  function registerCurrentView() {
    if (!view) return;
    registerEditor(view, {
      save: () => saveCurrentBuffer(),
      close: (force) => closeCurrentBuffer(force),
      closeAll: () => onClose(),
      saveAndClose: async () => { await saveCurrentBuffer(); closeCurrentBuffer(true); },
      nextBuffer: () => nextBuffer(),
      prevBuffer: () => prevBuffer(),
      onModeChange: (mode) => { vimMode = mode; },
    });
  }

  function ensureView(state: EditorState) {
    if (view) {
      unregisterEditor(view);
      view.destroy();
    }
    view = new EditorView({ state, parent: editorContainer });
    registerCurrentView();
  }

  let cleanupInterval: (() => void) | undefined;

  export async function openFile(filePath: string) {
    // Check if already open
    const existingIdx = buffers.findIndex((b) => b.path === filePath);
    if (existingIdx >= 0) {
      switchToBuffer(existingIdx);
      return;
    }

    const fullPath = filePath.startsWith("/") ? filePath : `${repoPath}/${filePath}`;
    try {
      const content = await git.readFile(fullPath, repoPath);
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
    onFileChange?.(buf.path.split("/").pop() || buf.path);
    onModifiedChange?.(buf.modified);

    if (buf.state) {
      ensureView(buf.state);
      applyLanguage(buf.path);
      view?.focus();
    }
  }

  function applyLanguage(filePath: string) {
    const langDesc = detectLanguageFromPath(filePath);
    if (!langDesc) {
      view?.dispatch({ effects: editorLangCompartment.reconfigure([]) });
      return;
    }
    langDesc.load().then((support) => {
      if (!view || buffers[activeIndex]?.path !== filePath) return;
      view.dispatch({ effects: editorLangCompartment.reconfigure(support.extension) });
    });
  }

  async function saveCurrentBuffer() {
    if (!activeBuffer || !view) return;
    const content = view.state.doc.toString();
    const fullPath = `${repoPath}/${activeBuffer.path}`;
    try {
      await git.writeFile(fullPath, content, repoPath);
      activeBuffer.modified = false;
      activeBuffer.content = content;
      onModifiedChange?.(false);
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
    if (view) {
      unregisterEditor(view);
      view.destroy();
    }
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
    view?.dispatch({ effects: editorThemeCompartment.reconfigure(themeExt) });
  });

  $effect(() => {
    const { font_family, font_size } = getSettings().terminal;
    view?.dispatch({ effects: editorFontCompartment.reconfigure(fontExtension(font_family, font_size)) });
  });

  function fileName(path: string): string {
    return path.split("/").pop() || path;
  }
</script>

<div class="flex flex-col h-full w-full" class:hidden={!visible}>
  <!-- Editor area -->
  <div bind:this={editorContainer} class="flex-1 min-w-0 relative overflow-hidden"></div>

  {#if !activeBuffer}
    <div class="absolute inset-0 flex items-center justify-center text-t3 bg-panel">
      <span class="text-sm">Press <kbd class="px-1.5 py-0.5 rounded bg-panel-hi text-xs font-mono">{MOD_LABEL}P</kbd> to open a file</span>
    </div>
  {/if}

  <!-- Status bar -->
  {#if activeBuffer}
    <div class="flex items-center justify-between px-3 py-0.5 text-xs font-mono border-t border-border bg-panel-hi text-t2">
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
          <span class="text-t3">[{activeIndex + 1}/{buffers.length}]</span>
        {/if}
        <span>{cursorLine}:{cursorCol}</span>
      </div>
    </div>
  {/if}
</div>
