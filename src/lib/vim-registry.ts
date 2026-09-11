/**
 * Vim Registry — routes global Vim ex commands and mode-change events
 * to the correct EditorTab instance based on the EditorView that fired them.
 *
 * Problem: @replit/codemirror-vim uses a global singleton for defineEx.
 * When multiple EditorTab instances exist (one per session), the last to call defineEx wins.
 *
 * Solution: Register ex commands once globally, then look up the correct handler set
 * via the cm.cm6 (EditorView) reference passed to each callback.
 * For vim-mode-change, register per-instance on the cm adapter.
 */
import { Vim, getCM } from "@replit/codemirror-vim";
import type { EditorView } from "@codemirror/view";

export interface VimHandlers {
  save: () => void;
  close: (force: boolean) => void;
  closeAll: () => void;
  saveAndClose: () => void;
  nextBuffer: () => void;
  prevBuffer: () => void;
  definition: () => void;
  references: () => void;
  rename: () => void;
  format: () => void;
  nextDiagnostic: () => void;
  previousDiagnostic: () => void;
  showDiagnostics: () => void;
  onModeChange: (mode: string) => void;
}

const registry = new Map<EditorView, VimHandlers>();
let initialized = false;

function initGlobal() {
  if (initialized) return;
  initialized = true;

  Vim.defineEx("w", "w", (cm: any) => {
    registry.get(cm.cm6)?.save();
  });
  Vim.defineEx("q", "q", (cm: any, params: any) => {
    registry.get(cm.cm6)?.close(!!params?.bang);
  });
  Vim.defineEx("qa", "qa", (cm: any) => {
    registry.get(cm.cm6)?.closeAll();
  });
  Vim.defineEx("wq", "wq", (cm: any) => {
    registry.get(cm.cm6)?.saveAndClose();
  });
  Vim.defineEx("bn", "bn", (cm: any) => {
    registry.get(cm.cm6)?.nextBuffer();
  });
  Vim.defineEx("bp", "bp", (cm: any) => {
    registry.get(cm.cm6)?.prevBuffer();
  });
  Vim.defineEx("LspDefinition", "LspDefinition", (cm: any) => {
    registry.get(cm.cm6)?.definition();
  });
  Vim.defineEx("LspReferences", "LspReferences", (cm: any) => {
    registry.get(cm.cm6)?.references();
  });
  Vim.defineEx("LspRename", "LspRename", (cm: any) => {
    registry.get(cm.cm6)?.rename();
  });
  Vim.defineEx("LspFormat", "LspFormat", (cm: any) => {
    registry.get(cm.cm6)?.format();
  });
  Vim.defineEx("LspNextDiagnostic", "LspNextDiagnostic", (cm: any) => {
    registry.get(cm.cm6)?.nextDiagnostic();
  });
  Vim.defineEx("LspPrevDiagnostic", "LspPrevDiagnostic", (cm: any) => {
    registry.get(cm.cm6)?.previousDiagnostic();
  });
  Vim.defineEx("LspDiagnostics", "LspDiagnostics", (cm: any) => {
    registry.get(cm.cm6)?.showDiagnostics();
  });

  Vim.map("gd", ":LspDefinition<CR>", "normal");
  Vim.map("gr", ":LspReferences<CR>", "normal");
  Vim.map("gR", ":LspRename<CR>", "normal");
  Vim.map("gF", ":LspFormat<CR>", "normal");
  Vim.map("]d", ":LspNextDiagnostic<CR>", "normal");
  Vim.map("[d", ":LspPrevDiagnostic<CR>", "normal");
  Vim.map("gD", ":LspDiagnostics<CR>", "normal");
}

function modeLabel(ev: { mode: string; subMode?: string }): string {
  if (ev.mode === "insert") return "INSERT";
  if (ev.mode === "visual") return ev.subMode === "linewise" ? "V-LINE" : "VISUAL";
  if (ev.mode === "replace") return "REPLACE";
  return "NORMAL";
}

export function registerEditor(view: EditorView, handlers: VimHandlers): void {
  initGlobal();
  registry.set(view, handlers);

  // Register per-instance vim-mode-change on the cm adapter
  const cm = getCM(view);
  if (cm) {
    const listener = (ev: { mode: string; subMode?: string }) => {
      handlers.onModeChange(modeLabel(ev));
    };
    cm.on("vim-mode-change", listener);
    (view as any)._vimModeListener = listener;
  }
}

export function unregisterEditor(view: EditorView): void {
  const cm = getCM(view);
  if (cm && (view as any)._vimModeListener) {
    cm.off("vim-mode-change", (view as any)._vimModeListener);
    delete (view as any)._vimModeListener;
  }
  registry.delete(view);
}
