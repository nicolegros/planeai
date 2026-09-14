import { describe, expect, it } from "vitest";
import {
  EDITOR_PRESETS,
  editorDraft,
  editorMode,
  validateEditorSettings,
} from "../editor-settings";

describe("editor settings", () => {
  it("defaults an absent editor configuration to the embedded editor", () => {
    expect(editorMode(undefined)).toBe("embedded");
    expect(editorDraft(undefined)).toEqual({ mode: "embedded", command: "", args: [] });
  });

  it("returns null for an unknown manually configured mode", () => {
    expect(editorMode({ mode: "unknown" as never, command: "tool", args: ["{file}"] })).toBeNull();
  });

  it("exposes editable presets with a file placeholder", () => {
    expect(EDITOR_PRESETS.map((preset) => preset.label)).toEqual([
      "VS Code",
      "Cursor",
      "Vim",
      "Neovim",
    ]);
    expect(
      EDITOR_PRESETS.every((preset) => preset.args.some((arg) => arg.includes("{file}"))),
    ).toBe(true);
  });

  it("requires an executable and file placeholder for terminal and external modes", () => {
    expect(validateEditorSettings({ mode: "external", command: "", args: ["{file}"] })).toBe(
      "Editor executable is required.",
    );
    expect(validateEditorSettings({ mode: "terminal", command: "nvim", args: [] })).toBe(
      "Editor arguments must include {file}.",
    );
    expect(
      validateEditorSettings({ mode: "terminal", command: "nvim", args: ["{file}"] }),
    ).toBeNull();
  });
});
