import { LSPPlugin } from "@codemirror/lsp-client";
import { getDialog, showDialog, type Command, type EditorView } from "@codemirror/view";
import { textEditsByUri, type LspWorkspaceEdit } from "./lsp-workspace-edit";

interface RenameParams {
  textDocument: { uri: string };
  position: { line: number; character: number };
  newName: string;
}

function applyRename(view: EditorView, newName: string): void {
  const plugin = LSPPlugin.get(view);
  const word = view.state.wordAt(view.state.selection.main.head);
  if (!plugin || !word) return;

  plugin.client.sync();
  void plugin.client.withMapping(async (mapping) => {
    const response = await plugin.client.request<RenameParams, LspWorkspaceEdit>("textDocument/rename", {
      textDocument: { uri: plugin.uri },
      position: plugin.toPosition(word.from),
      newName,
    });
    if (!response) return;

    for (const [uri, edits] of textEditsByUri(response)) {
      const file = plugin.client.workspace.getFile(uri);
      if (!file || edits.length === 0) continue;
      plugin.client.workspace.updateFile(uri, {
        changes: edits.map((edit) => ({
          from: mapping.mapPosition(uri, edit.range.start),
          to: mapping.mapPosition(uri, edit.range.end),
          insert: edit.newText,
        })),
        userEvent: "rename",
      });
    }
  }).catch((error) => plugin.reportError("Rename request failed", error));
}

/** Applies a server rename response, including rust-analyzer's documentChanges form. */
export const renameLspSymbol: Command = (view: EditorView): boolean => {
  const plugin = LSPPlugin.get(view);
  const word = view.state.wordAt(view.state.selection.main.head);
  if (!plugin || !word || plugin.client.serverCapabilities?.renameProvider === false) return false;

  const currentName = view.state.sliceDoc(word.from, word.to);
  const panel = getDialog(view, "cm-lsp-rename-panel");
  if (panel) {
    const input = panel.dom.querySelector<HTMLInputElement>("[name=name]");
    input?.focus();
    input?.select();
    return true;
  }

  const { close, result } = showDialog(view, {
    label: view.state.phrase("New name"),
    input: { name: "name", value: currentName },
    focus: true,
    submitLabel: view.state.phrase("rename"),
    class: "cm-lsp-rename-panel",
  });
  void result.then((form) => {
    view.dispatch({ effects: close });
    const input = form?.elements.namedItem("name");
    if (input instanceof HTMLInputElement) applyRename(view, input.value);
  });
  return true;
};
