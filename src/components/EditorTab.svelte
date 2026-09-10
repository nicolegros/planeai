<script lang="ts">
  import { git } from "../lib/api";
  import { LSPClient, languageServerExtensions } from "@codemirror/lsp-client";
  import { TauriLspTransport, fileUri } from "../lib/lsp-transport";
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
  import { isPlatformMod, MOD_ENTER_HINT, MOD_LABEL } from "../lib/keyboard";
  import { pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { recordUserInput } from "../lib/session-orchestrator.svelte";
  import {
    addEditorFeedback,
    beginEditorFeedbackSend,
    clearEditorFeedback,
    editEditorFeedback,
    endEditorFeedbackSend,
    getEditorFeedback,
    getEditorFeedbackCount,
    isEditorFeedbackSending,
    removeEditorFeedback,
    type EditorFeedback,
    type EditorFeedbackSnapshot,
  } from "../lib/editor-feedback.svelte";
  import { createEditorFeedbackSnapshot } from "../lib/editor-feedback-selection";
  import { serializeEditorFeedback } from "../lib/editor-feedback-serializer";

  interface LspSupport {
    client: LSPClient;
    transport: TauriLspTransport;
    documentUri: string;
  }

  interface Buffer {
    path: string;
    content: string;
    modified: boolean;
    state: EditorState | null;
    lsp: LspSupport | null;
  }

  interface Props {
    repoPath: string;
    sessionId: string;
    sessionExited?: boolean;
    visible: boolean;
    focused?: boolean;
    theme?: string;
    initialFile?: string;
    onClose: () => void;
    onFocusEditor: () => void;
    onFileChange?: (fileName: string) => void;
    onModifiedChange?: (modified: boolean) => void;
  }

  let { repoPath, sessionId, sessionExited = false, visible, focused = false, theme = "vs-dark", initialFile, onClose, onFocusEditor, onFileChange, onModifiedChange }: Props = $props();

  let buffers = $state<Buffer[]>([]);
  let activeIndex = $state(-1);
  let vimMode = $state("NORMAL");
  let cursorLine = $state(1);
  let cursorCol = $state(1);
  let editorContainer = $state<HTMLElement | undefined>(undefined);
  let view: EditorView | null = null;
  let mounted = $state(false);
  let initialFileOpened = $state(false);
  let hasSelection = $state(false);
  let showFeedbackComposer = $state(false);
  let showPendingFeedback = $state(false);
  let feedbackText = $state("");
  let feedbackSnapshot = $state<EditorFeedbackSnapshot | null>(null);
  let editingFeedbackId = $state<string | null>(null);
  let feedbackInputEl = $state<HTMLTextAreaElement>();
  let lspStatus = $state<"unavailable" | "connecting" | "ready">("unavailable");
  const sendingFeedback = $derived(isEditorFeedbackSending(sessionId));

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
  const pendingFeedback = $derived(getEditorFeedback(sessionId));
  const pendingFeedbackCount = $derived(getEditorFeedbackCount(sessionId));

  function createEditorState(content: string, filePath: string, lsp: LspSupport | null): EditorState {
    const themeExt = isDarkTheme(theme) ? darkTheme : lightTheme;
    const { font_family, font_size } = getSettings().terminal;
    const useVim = getSettings().vim_mode ?? true;

    return EditorState.create({
      doc: content,
      extensions: [
        EditorView.domEventHandlers({
          focus: () => {
            onFocusEditor();
          },
        }),
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
          { key: "Mod-Shift-c", run: () => { openFeedbackComposer(); return true; } },
        ])),
        ...(useVim ? [vim()] : []),
        basicSetup,
        syntaxHighlighting(classHighlighter),
        ...(useVim ? [] : [keymap.of(defaultKeymap)]),
        editorThemeCompartment.of(themeExt),
        editorFontCompartment.of(fontExtension(font_family, font_size)),
        editorLangCompartment.of([]),
        ...(lsp ? [lsp.client.plugin(lsp.documentUri, lsp.transport.languageId)] : []),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && activeBuffer) {
            if (!activeBuffer.modified) onModifiedChange?.(true);
            activeBuffer.modified = true;
          }
          if (update.selectionSet || update.docChanged) {
            const selection = update.state.selection.main;
            hasSelection = !selection.empty;
            const pos = selection.head;
            const line = update.state.doc.lineAt(pos);
            cursorLine = line.number;
            cursorCol = pos - line.from + 1;
          }
        }),
      ],
    });
  }

  async function connectLsp(fullPath: string): Promise<LspSupport | null> {
    lspStatus = "connecting";
    try {
      const transport = await TauriLspTransport.connect(repoPath, fullPath);
      const client = new LSPClient({
        rootUri: fileUri(repoPath),
        extensions: languageServerExtensions(),
      }).connect(transport);
      await client.initializing;
      lspStatus = "ready";
      return { client, transport, documentUri: fileUri(fullPath) };
    } catch (error) {
      console.info("LSP unavailable for file:", error);
      lspStatus = "unavailable";
      return null;
    }
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
    if (!editorContainer) {
      if (import.meta.env.DEV) console.warn("ensureView called before editorContainer is bound");
      return;
    }
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
      // Guard: component may have unmounted during async fetch
      if (!editorContainer || !mounted) return;
      const lsp = await connectLsp(fullPath);
      if (!editorContainer || !mounted) {
        if (lsp) {
          lsp.client.disconnect();
          await lsp.transport.close();
        }
        return;
      }
      const state = createEditorState(content, filePath, lsp);
      buffers.push({ path: filePath, content, modified: false, state, lsp });
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
    lspStatus = buf.lsp ? "ready" : "unavailable";
    onFileChange?.(buf.path.split("/").pop() || buf.path);
    onModifiedChange?.(buf.modified);

    if (buf.state) {
      hasSelection = !buf.state.selection.main.empty;
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

  async function closeLsp(buffer: Buffer) {
    if (!buffer.lsp) return;
    buffer.lsp.client.disconnect();
    await buffer.lsp.transport.close();
    buffer.lsp = null;
  }

  function openFeedbackComposer(): void {
    if (sendingFeedback || showFeedbackComposer || !activeBuffer || !view) return;
    const selection = view.state.selection.main;
    const result = createEditorFeedbackSnapshot({
      doc: view.state.doc,
      from: selection.from,
      to: selection.to,
      filePath: activeBuffer.path,
      language: detectLanguageFromPath(activeBuffer.path)?.name ?? "",
      isUnsaved: activeBuffer.modified,
    });
    if ("error" in result) {
      showSnackbar(
        result.error === "empty"
          ? "Select text before adding editor feedback."
          : "Select at most 200 lines or 12 KiB of text for editor feedback.",
        "error",
      );
      return;
    }
    feedbackSnapshot = result.snapshot;
    feedbackText = "";
    editingFeedbackId = null;
    showFeedbackComposer = true;
    requestAnimationFrame(() => feedbackInputEl?.focus());
  }

  function discardFeedbackDraft(): boolean {
    if (!showFeedbackComposer || !feedbackText.trim()) { cancelFeedbackComposer(); return true; }
    if (!window.confirm("Discard the unsaved editor feedback?")) return false;
    cancelFeedbackComposer();
    return true;
  }

  function openFeedbackEdit(feedback: EditorFeedback): void {
    if (sendingFeedback) return;
    if (showFeedbackComposer && editingFeedbackId === feedback.id) {
      feedbackInputEl?.focus();
      return;
    }
    if (showFeedbackComposer && !discardFeedbackDraft()) return;
    feedbackSnapshot = null;
    feedbackText = feedback.text;
    editingFeedbackId = feedback.id;
    showFeedbackComposer = true;
    requestAnimationFrame(() => feedbackInputEl?.focus());
  }

  function cancelFeedbackComposer(): void {
    showFeedbackComposer = false;
    feedbackSnapshot = null;
    feedbackText = "";
    editingFeedbackId = null;
  }

  function submitFeedback(): void {
    const text = feedbackText.trim();
    if (!text || sendingFeedback) return;
    if (editingFeedbackId) {
      editEditorFeedback(sessionId, editingFeedbackId, text);
    } else if (feedbackSnapshot) {
      addEditorFeedback(sessionId, { ...feedbackSnapshot, text });
      showPendingFeedback = true;
      view?.dispatch({ selection: { anchor: view.state.selection.main.head } });
    } else return;
    cancelFeedbackComposer();
  }

  function handleFeedbackKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") { event.preventDefault(); discardFeedbackDraft(); }
    else if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submitFeedback(); }
  }

  async function sendFeedback(): Promise<void> {
    if (pendingFeedback.length === 0 || sessionExited || sendingFeedback || showFeedbackComposer) return;
    if (!beginEditorFeedbackSend(sessionId)) return;
    try {
      const bytes = Array.from(new TextEncoder().encode(serializeEditorFeedback(pendingFeedback)));
      bytes.push(0x0d);
      const delivered = await pty.write(sessionId, bytes);
      if (!delivered) throw new Error("PTY session is not attached. Try again once it is ready.");
      recordUserInput(sessionId);
      const count = pendingFeedback.length;
      clearEditorFeedback(sessionId);
      showSnackbar(`Editor feedback sent (${count} note${count === 1 ? "" : "s"})`, "success");
    } catch (error) {
      showSnackbar(String(error).replace(/^Error: /, ""), "error");
    } finally {
      endEditorFeedbackSend(sessionId);
    }
  }

  function isShortcutHandledByControl(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    const editable = target.closest("[contenteditable]");
    return Boolean(
      target.closest("input, textarea, select, button, [role='dialog']")
      || (editable && !editable.classList.contains("cm-content")),
    );
  }

  function handleFeedbackShortcut(event: KeyboardEvent): void {
    if (
      !visible
      || !focused
      || event.defaultPrevented
      || isShortcutHandledByControl(event.target)
      || event.key !== "Enter"
      || !isPlatformMod(event)
      || pendingFeedback.length === 0
      || sessionExited
      || sendingFeedback
      || showFeedbackComposer
    ) return;

    event.preventDefault();
    event.stopPropagation();
    void sendFeedback();
  }

  function closeCurrentBuffer(force: boolean) {
    if (!activeBuffer) return;
    if (activeBuffer.modified && !force) {
      console.warn("Buffer has unsaved changes. Use :q! to force.");
      return;
    }
    void closeLsp(activeBuffer);
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
    window.addEventListener("keydown", handleFeedbackShortcut, true);
    return () => window.removeEventListener("keydown", handleFeedbackShortcut, true);
  });

  onDestroy(() => {
    cleanupInterval?.();
    for (const buffer of buffers) void closeLsp(buffer);
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
    if (focused && view) {
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

<div class="flex flex-col h-full w-full" class:hidden={!visible} data-editor-tab>
  {#if showFeedbackComposer}
    <div class="shrink-0 border-b border-border bg-chrome p-3">
      <div class="mb-1.5 text-[10px] text-t3">
        ● {editingFeedbackId ? "Edit editor feedback" : `Editor feedback · ${feedbackSnapshot?.filePath ?? ""} · ${feedbackSnapshot?.startLine === feedbackSnapshot?.endLine ? `line ${feedbackSnapshot?.startLine}` : `lines ${feedbackSnapshot?.startLine}–${feedbackSnapshot?.endLine}`}`}
      </div>
      <textarea bind:this={feedbackInputEl} bind:value={feedbackText} onkeydown={handleFeedbackKeydown} aria-label="Editor feedback comment" class="w-full resize-none rounded-lg border border-border-s bg-panel p-2 text-[12.5px] text-t1 focus:outline-none focus:ring-1 focus:ring-accent" rows="3" placeholder="Add a note… (Enter to submit, Esc to cancel)"></textarea>
    </div>
  {/if}

  {#if pendingFeedbackCount > 0}
    <div class="shrink-0 border-b border-border bg-chrome">
      <button type="button" class="flex w-full items-center justify-between px-3 py-2 text-left text-[11px] text-t2 hover:bg-panel-hi hover:text-t1" onclick={() => (showPendingFeedback = !showPendingFeedback)} aria-expanded={showPendingFeedback}>
        <span>Pending editor feedback ({pendingFeedbackCount})</span><span>{showPendingFeedback ? "−" : "+"}</span>
      </button>
      {#if showPendingFeedback}
        <div class="max-h-40 overflow-y-auto border-t border-border">
          {#each pendingFeedback as feedback (feedback.id)}
            <div class="flex items-start gap-2 border-b border-border/50 px-3 py-2 last:border-b-0" role="group" aria-label="Pending editor feedback">
              <button type="button" class="min-w-0 flex-1 text-left" onclick={() => openFeedbackEdit(feedback)}>
                <span class="mr-2 font-mono text-[10px] text-t3">{feedback.filePath} L{feedback.startLine}{feedback.startLine === feedback.endLine ? "" : `–${feedback.endLine}`}{feedback.isUnsaved ? " · unsaved" : ""}</span>
                <span class="block truncate font-mono text-[10px] text-t3">{feedback.selectedText.split("\n")[0]}</span>
                <span class="whitespace-pre-wrap text-[12px] text-t1">{feedback.text}</span>
              </button>
              <button class="text-[11px] text-t3 hover:text-t1" onclick={() => openFeedbackEdit(feedback)} disabled={sendingFeedback} title="Edit note" aria-label="Edit editor feedback">✎</button>
              <button class="text-[13px] text-t3 hover:text-t1" onclick={() => { if (!sendingFeedback) removeEditorFeedback(sessionId, feedback.id); }} disabled={sendingFeedback} title="Delete note" aria-label="Delete editor feedback">×</button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

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
        <button type="button" class="rounded px-1.5 py-0.5 text-[10px] text-t2 hover:bg-panel hover:text-t1 disabled:cursor-not-allowed disabled:opacity-50" onclick={openFeedbackComposer} disabled={!hasSelection || sendingFeedback || showFeedbackComposer} title={`Comment selection (${MOD_LABEL}⇧C)`}>Comment</button>
        {#if pendingFeedbackCount > 0}
          <button type="button" class="rounded bg-accent-bg px-1.5 py-0.5 text-[10px] text-accent hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50" onclick={() => void sendFeedback()} disabled={sessionExited || sendingFeedback || showFeedbackComposer} title={sessionExited ? "Agent is not running" : showFeedbackComposer ? "Submit or cancel the open comment before sending feedback" : `Send feedback (${MOD_ENTER_HINT})`}>Send ({pendingFeedbackCount})</button>
        {/if}
        <span class={lspStatus === "ready" ? "text-green-500" : lspStatus === "connecting" ? "text-yellow-400" : "text-t3"}>LSP {lspStatus}</span>
        {#if buffers.length > 1}
          <span class="text-t3">[{activeIndex + 1}/{buffers.length}]</span>
        {/if}
        <span>{cursorLine}:{cursorCol}</span>
      </div>
    </div>
  {/if}
</div>
