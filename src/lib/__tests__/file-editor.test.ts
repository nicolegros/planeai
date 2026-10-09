import { describe, expect, it, vi } from "vitest";
import {
  openFileWithConfiguredEditor,
  workingDirRelativePath,
  type FileEditorOpenHandlers,
} from "../file-editor";
import type { EditorSettings } from "../settings.svelte";

function handlers(): FileEditorOpenHandlers {
  return {
    openEmbedded: vi.fn(),
    openTerminal: vi.fn(),
    openExternal: vi.fn(),
  };
}

const editorModes: Array<[EditorSettings | null, keyof FileEditorOpenHandlers]> = [
  [null, "openEmbedded"],
  [{ mode: "embedded", command: "", args: [] }, "openEmbedded"],
  [{ mode: "terminal", command: "nvim", args: ["{file}"] }, "openTerminal"],
  [{ mode: "external", command: "code", args: ["{file}"] }, "openExternal"],
];

describe("openFileWithConfiguredEditor", () => {
  it.each(editorModes)("routes %j through %s", async (editor, expectedHandler) => {
    const actions = handlers();

    await expect(openFileWithConfiguredEditor(editor, actions)).resolves.toBe("opened");

    expect(actions[expectedHandler]).toHaveBeenCalledOnce();
    for (const [name, handler] of Object.entries(actions)) {
      if (name !== expectedHandler) expect(handler).not.toHaveBeenCalled();
    }
  });

  it("blocks an unknown persisted editor mode without opening a file", async () => {
    const actions = handlers();

    await expect(
      openFileWithConfiguredEditor(
        { mode: "unsupported", command: "code", args: ["{file}"] } as unknown as EditorSettings,
        actions,
      ),
    ).resolves.toBe("invalid");

    expect(actions.openEmbedded).not.toHaveBeenCalled();
    expect(actions.openTerminal).not.toHaveBeenCalled();
    expect(actions.openExternal).not.toHaveBeenCalled();
  });
});

describe("workingDirRelativePath", () => {
  it("stores files inside the session working dir relative to it", () => {
    expect(workingDirRelativePath("/repos/app/src/main.rs", "/repos/app")).toBe("src/main.rs");
    expect(workingDirRelativePath("/repos/app/src/main.rs", "/repos/app/")).toBe("src/main.rs");
  });

  it("keeps paths outside the working dir, already relative, or without one", () => {
    expect(workingDirRelativePath("/repos/application/a.ts", "/repos/app")).toBe(
      "/repos/application/a.ts",
    );
    expect(workingDirRelativePath("/etc/hosts", "/repos/app")).toBe("/etc/hosts");
    expect(workingDirRelativePath("src/main.rs", "/repos/app")).toBe("src/main.rs");
    expect(workingDirRelativePath("/repos/app/x.ts", null)).toBe("/repos/app/x.ts");
  });
});
