import { EditorView, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from "@codemirror/view";
import { Compartment } from "@codemirror/state";
import { languages } from "@codemirror/language-data";
import { LanguageDescription, syntaxHighlighting, defaultHighlightStyle, foldGutter } from "@codemirror/language";

export const fontCompartment = new Compartment();
export const themeCompartment = new Compartment();
export const langCompartment = new Compartment();

export const baseExtensions = [
  lineNumbers(),
  foldGutter(),
  highlightActiveLine(),
  highlightActiveLineGutter(),
  syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
];

export const darkTheme = EditorView.theme(
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
    ".cm-fat-cursor": {
      background: "rgba(255, 255, 255, 0.35) !important",
      outline: "1px solid rgba(255, 255, 255, 0.55) !important",
    },
    "&:not(.cm-focused) .cm-fat-cursor": {
      background: "transparent !important",
      outline: "1px solid rgba(255, 255, 255, 0.35) !important",
    },
  },
  { dark: true },
);

export const lightTheme = EditorView.theme({
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
  ".cm-fat-cursor": {
    background: "rgba(0, 0, 0, 0.3) !important",
    outline: "1px solid rgba(0, 0, 0, 0.5) !important",
  },
  "&:not(.cm-focused) .cm-fat-cursor": {
    background: "transparent !important",
    outline: "1px solid rgba(0, 0, 0, 0.3) !important",
  },
});

export function fontExtension(family: string, size: number) {
  return EditorView.theme({
    "&": { fontSize: `${size}px` },
    ".cm-content, .cm-gutters": { fontFamily: family },
  });
}

export function isDarkTheme(theme: string): boolean {
  return theme.includes("dark") || theme.includes("black");
}

export function findLanguage(lang: string): LanguageDescription | null {
  return LanguageDescription.matchLanguageName(languages, lang, true);
}

export function detectLanguageFromPath(filePath: string): LanguageDescription | null {
  const filename = filePath.split("/").pop() || filePath;
  return LanguageDescription.matchFilename(languages, filename);
}
