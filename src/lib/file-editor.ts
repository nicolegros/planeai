import { editorMode } from "./editor-settings";
import type { EditorSettings } from "./settings.svelte";

export interface FileEditorOpenHandlers {
  openEmbedded: () => void | Promise<void>;
  openTerminal: () => void | Promise<void>;
  openExternal: () => void | Promise<void>;
}

/** Route a file-open request through the configured global editor mode. */
export async function openFileWithConfiguredEditor(
  editor: EditorSettings | null | undefined,
  handlers: FileEditorOpenHandlers,
): Promise<"opened" | "invalid"> {
  const mode = editorMode(editor);
  if (!mode) return "invalid";

  switch (mode) {
    case "embedded":
      await handlers.openEmbedded();
      break;
    case "terminal":
      await handlers.openTerminal();
      break;
    case "external":
      await handlers.openExternal();
      break;
  }
  return "opened";
}

/**
 * The path an editor tab stores for `filePath`: relative to the session's working dir when
 * inside it, so saved layouts survive the project folder moving.
 */
export function workingDirRelativePath(
  filePath: string,
  workingDir: string | null | undefined,
): string {
  if (!workingDir) return filePath;
  const root = workingDir.replace(/[/\\]+$/, "");
  for (const separator of ["/", "\\"]) {
    if (filePath.startsWith(root + separator) && filePath.length > root.length + 1) {
      return filePath.slice(root.length + 1);
    }
  }
  return filePath;
}
