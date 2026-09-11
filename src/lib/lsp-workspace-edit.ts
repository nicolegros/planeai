export interface LspPosition {
  line: number;
  character: number;
}

export interface LspTextEdit {
  range: { start: LspPosition; end: LspPosition };
  newText: string;
}

interface TextDocumentEdit {
  textDocument?: { uri?: string };
  edits?: LspTextEdit[];
}

export interface LspWorkspaceEdit {
  changes?: Record<string, LspTextEdit[]>;
  documentChanges?: TextDocumentEdit[];
}

/**
 * Normalizes the two WorkspaceEdit representations used by language servers.
 * Resource operations are intentionally omitted: PlaneAI currently updates
 * only open editor documents and does not yet preview file operations.
 */
export function textEditsByUri(workspaceEdit: LspWorkspaceEdit): Map<string, LspTextEdit[]> {
  const editsByUri = new Map<string, LspTextEdit[]>();

  for (const [uri, edits] of Object.entries(workspaceEdit.changes ?? {})) {
    editsByUri.set(uri, [...edits]);
  }

  for (const change of workspaceEdit.documentChanges ?? []) {
    const uri = change.textDocument?.uri;
    if (!uri || !change.edits?.length) continue;
    editsByUri.set(uri, [...(editsByUri.get(uri) ?? []), ...change.edits]);
  }

  return editsByUri;
}
