import type { DiffRenderer } from "./diff-renderer";
import { MergeView, goToNextChunk, goToPreviousChunk } from "@codemirror/merge";
import {
  EditorView,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
} from "@codemirror/view";
import { EditorState, Compartment } from "@codemirror/state";
import { languages } from "@codemirror/language-data";
import {
  LanguageDescription,
  syntaxHighlighting,
  defaultHighlightStyle,
  foldGutter,
} from "@codemirror/language";

// Minimal, read-only feature set. We intentionally avoid `basicSetup` because it
// bundles history, autocompletion, search, linting and bracket matching — none of
// which a read-only diff needs, and all of which add construction cost (×2 editors).
const readOnlyExtensions = [
  lineNumbers(),
  foldGutter(),
  highlightActiveLine(),
  highlightActiveLineGutter(),
  syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
  EditorState.readOnly.of(true),
  EditorView.editable.of(false),
];

const fontCompartment = new Compartment();
const themeCompartment = new Compartment();
const langCompartment = new Compartment();

const darkTheme = EditorView.theme(
  {
    "&": { backgroundColor: "var(--editor-background)" },
    ".cm-gutters": {
      backgroundColor: "var(--editor-background)",
      color: "var(--editor-line-number)",
      borderRight: "1px solid var(--color-surface-300)",
    },
    ".cm-content": { color: "var(--editor-foreground)" },
    ".cm-activeLine": { backgroundColor: "var(--editor-selection)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor: "var(--editor-selection)",
    },
    ".cm-changedLine.cm-insertedLine": { backgroundColor: "var(--editor-added-bg)" },
    ".cm-changedLine.cm-deletedLine": { backgroundColor: "var(--editor-deleted-bg)" },
  },
  { dark: true },
);

const lightTheme = EditorView.theme({
  "&": { backgroundColor: "var(--editor-background)" },
  ".cm-gutters": {
    backgroundColor: "var(--editor-background)",
    color: "var(--editor-line-number)",
    borderRight: "1px solid var(--color-surface-300)",
  },
  ".cm-content": { color: "var(--editor-foreground)" },
  ".cm-activeLine": { backgroundColor: "var(--editor-selection)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "var(--editor-selection)",
  },
  ".cm-changedLine.cm-insertedLine": { backgroundColor: "var(--editor-added-bg)" },
  ".cm-changedLine.cm-deletedLine": { backgroundColor: "var(--editor-deleted-bg)" },
});

function fontExtension(family: string, size: number) {
  return EditorView.theme({
    "&": { fontSize: `${size}px` },
    ".cm-content, .cm-gutters": { fontFamily: family },
  });
}

function isDarkTheme(theme: string): boolean {
  return theme.includes("dark") || theme.includes("black");
}

function findLanguage(lang: string): LanguageDescription | null {
  return LanguageDescription.matchLanguageName(languages, lang, true);
}

export class CmDiffRenderer implements DiffRenderer {
  private container: HTMLElement | null = null;
  private mergeView: MergeView | null = null;
  private currentTheme = "vs-dark";
  private currentMode: "side-by-side" | "unified" = "side-by-side";
  private fontFamily = "Menlo";
  private fontSize = 14;
  private original = "";
  private modified = "";
  private language = "";

  mount(container: HTMLElement): void {
    this.container = container;
  }

  setDiff(original: string, modified: string, language: string): void {
    this.original = original;
    this.modified = modified;
    this.language = language;
    // Always rebuild: dispatching to a and b sequentially causes an intermediate
    // state where chunks are computed against mismatched documents, breaking
    // collapseUnchanged decorations that don't recover after the second dispatch.
    this.rebuild();
  }

  setTheme(theme: string): void {
    if (theme === this.currentTheme && this.mergeView) return;
    this.currentTheme = theme;
    // Theme is a compartment, so swap it without rebuilding the view.
    if (this.mergeView) {
      const ext = isDarkTheme(theme) ? darkTheme : lightTheme;
      this.mergeView.a.dispatch({ effects: themeCompartment.reconfigure(ext) });
      this.mergeView.b.dispatch({ effects: themeCompartment.reconfigure(ext) });
    }
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

  setMode(mode: "side-by-side" | "unified"): void {
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

  private applyLanguage() {
    const langDesc = findLanguage(this.language);
    if (!langDesc) {
      if (this.mergeView) {
        this.mergeView.a.dispatch({ effects: langCompartment.reconfigure([]) });
        this.mergeView.b.dispatch({ effects: langCompartment.reconfigure([]) });
      }
      return;
    }
    const targetLanguage = this.language;
    langDesc.load().then((support) => {
      // Guard against a newer file having been selected while loading.
      if (!this.mergeView || this.language !== targetLanguage) return;
      this.mergeView.a.dispatch({ effects: langCompartment.reconfigure(support.extension) });
      this.mergeView.b.dispatch({ effects: langCompartment.reconfigure(support.extension) });
    });
  }

  private rebuild() {
    if (!this.container) return;
    this.mergeView?.destroy();
    this.container.innerHTML = "";

    const themeExt = isDarkTheme(this.currentTheme) ? darkTheme : lightTheme;

    const sharedExtensions = [
      readOnlyExtensions,
      themeCompartment.of(themeExt),
      fontCompartment.of(fontExtension(this.fontFamily, this.fontSize)),
      langCompartment.of([]),
    ];

    this.mergeView = new MergeView({
      a: { doc: this.original, extensions: sharedExtensions },
      b: { doc: this.modified, extensions: sharedExtensions },
      parent: this.container,
      collapseUnchanged: { margin: 3, minSize: 4 },
      highlightChanges: true,
      gutter: true,
    });

    this.mergeView.dom.style.height = "100%";
    this.mergeView.dom.style.overflow = "auto";

    this.applyLanguage();
  }
}
