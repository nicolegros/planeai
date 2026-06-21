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
