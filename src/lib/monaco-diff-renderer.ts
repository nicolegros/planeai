import type { DiffRenderer } from './diff-renderer';

let monacoModule: typeof import('monaco-editor') | null = null;

async function loadMonaco() {
  if (monacoModule) return monacoModule;
  const monaco = await import('monaco-editor');

  // Configure worker
  self.MonacoEnvironment = {
    getWorker(_workerId: string, _label: string) {
      return new Worker(
        new URL('monaco-editor/esm/vs/editor/editor.worker.js', import.meta.url),
        { type: 'module' }
      );
    }
  };

  monacoModule = monaco;
  return monaco;
}

export class MonacoDiffRenderer implements DiffRenderer {
  private editor: import('monaco-editor').editor.IStandaloneDiffEditor | null = null;
  private originalModel: import('monaco-editor').editor.ITextModel | null = null;
  private modifiedModel: import('monaco-editor').editor.ITextModel | null = null;
  private container: HTMLElement | null = null;
  private currentMode: 'side-by-side' | 'unified' = 'side-by-side';

  mount(container: HTMLElement): void {
    this.container = container;
    this.initEditor();
  }

  private async initEditor() {
    if (!this.container) return;
    const monaco = await loadMonaco();

    this.editor = monaco.editor.createDiffEditor(this.container, {
      readOnly: true,
      renderSideBySide: true,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
    });
  }

  setDiff(original: string, modified: string, language: string): void {
    this.setDiffAsync(original, modified, language);
  }

  private async setDiffAsync(original: string, modified: string, language: string) {
    const monaco = await loadMonaco();
    if (!this.editor) return;

    this.originalModel?.dispose();
    this.modifiedModel?.dispose();

    this.originalModel = monaco.editor.createModel(original, language);
    this.modifiedModel = monaco.editor.createModel(modified, language);

    this.editor.setModel({
      original: this.originalModel,
      modified: this.modifiedModel,
    });
  }

  setTheme(theme: string): void {
    this.setThemeAsync(theme);
  }

  private async setThemeAsync(theme: string) {
    const monaco = await loadMonaco();
    monaco.editor.setTheme(theme);
  }

  setMode(mode: 'side-by-side' | 'unified'): void {
    this.currentMode = mode;
    this.editor?.updateOptions({ renderSideBySide: mode === 'side-by-side' });
  }

  navigateNext(): void {
    if (!this.editor) return;
    const changes = (this.editor as any).getLineChanges?.() ?? [];
    // Use built-in diff navigator action
    this.editor.getModifiedEditor().trigger('diff', 'editor.action.diffReview.next', {});
  }

  navigatePrevious(): void {
    if (!this.editor) return;
    this.editor.getModifiedEditor().trigger('diff', 'editor.action.diffReview.prev', {});
  }

  destroy(): void {
    this.originalModel?.dispose();
    this.modifiedModel?.dispose();
    this.editor?.dispose();
    this.editor = null;
    this.originalModel = null;
    this.modifiedModel = null;
    this.container = null;
  }
}
