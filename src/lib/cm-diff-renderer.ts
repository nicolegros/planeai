import type { DiffRenderer } from './diff-renderer';
import { MergeView, goToNextChunk, goToPreviousChunk } from '@codemirror/merge';
import { EditorView } from '@codemirror/view';
import { EditorState, Compartment } from '@codemirror/state';
import { basicSetup } from 'codemirror';
import { languages } from '@codemirror/language-data';
import { LanguageDescription } from '@codemirror/language';

const fontCompartment = new Compartment();

const darkTheme = EditorView.theme({
  '&': { backgroundColor: '#1e1e1e' },
  '.cm-gutters': { backgroundColor: '#1e1e1e', borderRight: '1px solid #333' },
}, { dark: true });

const lightTheme = EditorView.theme({
  '&': { backgroundColor: '#ffffff' },
  '.cm-gutters': { backgroundColor: '#ffffff' },
});

function fontExtension(family: string, size: number) {
  return EditorView.theme({
    '&': { fontSize: `${size}px` },
    '.cm-content, .cm-gutters': { fontFamily: family },
  });
}

function isDarkTheme(theme: string): boolean {
  return theme.includes('dark') || theme.includes('black');
}

function findLanguage(lang: string): LanguageDescription | undefined {
  return LanguageDescription.matchLanguageName(languages, lang, true);
}

export class CmDiffRenderer implements DiffRenderer {
  private container: HTMLElement | null = null;
  private mergeView: MergeView | null = null;
  private currentTheme = 'vs-dark';
  private currentMode: 'side-by-side' | 'unified' = 'side-by-side';
  private fontFamily = 'Menlo';
  private fontSize = 14;
  private original = '';
  private modified = '';
  private language = '';

  mount(container: HTMLElement): void {
    this.container = container;
  }

  setDiff(original: string, modified: string, language: string): void {
    this.original = original;
    this.modified = modified;
    this.language = language;
    this.rebuild();
  }

  setTheme(theme: string): void {
    this.currentTheme = theme;
    this.rebuild();
  }

  setFont(family: string, size: number): void {
    this.fontFamily = family;
    this.fontSize = size;
    if (this.mergeView) {
      const ext = fontExtension(family, size);
      this.mergeView.a.dispatch({ effects: fontCompartment.reconfigure(ext) });
      this.mergeView.b.dispatch({ effects: fontCompartment.reconfigure(ext) });
    }
  }

  setMode(mode: 'side-by-side' | 'unified'): void {
    this.currentMode = mode;
    this.rebuild();
  }

  navigateNext(): void {
    if (!this.mergeView) return;
    const view = this.mergeView.b;
    goToNextChunk({ state: view.state, dispatch: view.dispatch.bind(view) });
  }

  navigatePrevious(): void {
    if (!this.mergeView) return;
    const view = this.mergeView.b;
    goToPreviousChunk({ state: view.state, dispatch: view.dispatch.bind(view) });
  }

  destroy(): void {
    this.mergeView?.destroy();
    this.mergeView = null;
    this.container = null;
  }

  private rebuild() {
    if (!this.container) return;
    this.mergeView?.destroy();
    this.container.innerHTML = '';

    const dark = isDarkTheme(this.currentTheme);
    const themeExt = dark ? darkTheme : lightTheme;
    const font = fontCompartment.of(fontExtension(this.fontFamily, this.fontSize));

    const sharedExtensions = [
      basicSetup,
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      themeExt,
      font,
    ];

    const langDesc = findLanguage(this.language);
    if (langDesc) {
      langDesc.load().then(support => {
        if (!this.mergeView) return;
        this.mergeView.a.dispatch({ effects: EditorState.appendConfig.of(support.extension) });
        this.mergeView.b.dispatch({ effects: EditorState.appendConfig.of(support.extension) });
      });
    }

    this.mergeView = new MergeView({
      a: { doc: this.original, extensions: sharedExtensions },
      b: { doc: this.modified, extensions: sharedExtensions },
      parent: this.container,
      collapseUnchanged: { margin: 3, minSize: 4 },
      highlightChanges: true,
      gutter: true,
    });

    this.mergeView.dom.style.height = '100%';
    this.mergeView.dom.style.overflow = 'auto';
  }
}
