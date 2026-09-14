import type { EditorSettings } from "./settings.svelte";

export type EditorMode = "embedded" | "terminal" | "external";

export interface EditorPreset {
  label: string;
  command: string;
  args: string[];
}

export const EDITOR_PRESETS: readonly EditorPreset[] = [
  { label: "VS Code", command: "code", args: ["--reuse-window", "--goto", "{file}"] },
  { label: "Cursor", command: "cursor", args: ["--reuse-window", "--goto", "{file}"] },
  { label: "Vim", command: "vim", args: ["{file}"] },
  { label: "Neovim", command: "nvim", args: ["{file}"] },
];

export function editorMode(editor: EditorSettings | null | undefined): EditorMode | null {
  if (!editor) return "embedded";
  if (editor.mode === "embedded" || editor.mode === "terminal" || editor.mode === "external") {
    return editor.mode;
  }
  return null;
}

export function editorDraft(
  editor: EditorSettings | null | undefined,
  mode: EditorMode | null = editorMode(editor),
): EditorSettings {
  return {
    mode: mode ?? "embedded",
    command: editor?.command ?? "",
    args: [...(editor?.args ?? [])],
  };
}

export function validateEditorSettings(editor: EditorSettings): string | null {
  if (editor.mode === "embedded") return null;
  if (!editor.command.trim()) return "Editor executable is required.";
  if (!editor.args.some((arg) => arg.includes("{file}"))) {
    return "Editor arguments must include {file}.";
  }
  return null;
}
