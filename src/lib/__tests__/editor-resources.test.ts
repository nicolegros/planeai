import { beforeEach, describe, expect, it, vi } from "vitest";
import * as tree from "../layout-tree";
import type { Layout, TabEntry } from "../layout-tree";

const workspace = vi.hoisted(() => ({ layout: null as Layout | null }));
vi.mock("../task-workspace-layout.svelte", async () => {
  const { focusedTabOf } = await import("../layout-tree");
  return { taskWorkspaceLayout: { focusedTab: () => focusedTabOf(workspace.layout) } };
});
import {
  _resetEditorResources,
  activeEditorResourceKey,
  registerEditorResource,
  saveActiveEditorResource,
  unregisterEditorResource,
} from "../editor-resources";

const SESSION = "agent-1";
const EDITOR_KEY = `${SESSION}:editor:src/main.rs`;
const OTHER_EDITOR_KEY = `${SESSION}:editor:src/lib.rs`;

const agent: TabEntry = { ptyKey: SESSION, label: "Agent", icon: "bot", type: "agent" };
const editor: TabEntry = {
  ptyKey: EDITOR_KEY,
  label: "main.rs",
  icon: "file",
  type: "editor",
  filePath: "src/main.rs",
};

beforeEach(() => {
  workspace.layout = null;
  _resetEditorResources();
});

describe("activeEditorResourceKey", () => {
  it("returns the front editor tab of the focused pane", () => {
    workspace.layout = tree.createLayout([agent, editor], EDITOR_KEY);
    expect(activeEditorResourceKey()).toBe(EDITOR_KEY);
  });

  it("returns null when the front tab is not an editor", () => {
    workspace.layout = tree.createLayout([agent, editor], SESSION);
    expect(activeEditorResourceKey()).toBeNull();
  });

  it("returns null when there is no layout", () => {
    expect(activeEditorResourceKey()).toBeNull();
  });

  it("ignores an editor tab that is open in an unfocused pane", () => {
    const split = tree.splitFocused(tree.createLayout([agent], SESSION), "vertical")!;
    const withEditor = tree.addTab(split.layout, split.leafId, editor);
    workspace.layout = tree.focusTab(withEditor, SESSION);

    expect(activeEditorResourceKey()).toBeNull();
  });
});

describe("saveActiveEditorResource", () => {
  it("saves the editor tab in the focused pane", () => {
    workspace.layout = tree.createLayout([agent, editor], EDITOR_KEY);
    const save = vi.fn();
    registerEditorResource(EDITOR_KEY, { save });

    expect(saveActiveEditorResource()).toBe(true);
    expect(save).toHaveBeenCalledOnce();
  });

  it("saves the file in front, not another open file of the same session", () => {
    workspace.layout = tree.createLayout(
      [agent, { ...editor, ptyKey: OTHER_EDITOR_KEY }, editor],
      EDITOR_KEY,
    );
    const front = vi.fn();
    const other = vi.fn();
    registerEditorResource(EDITOR_KEY, { save: front });
    registerEditorResource(OTHER_EDITOR_KEY, { save: other });

    expect(saveActiveEditorResource()).toBe(true);
    expect(front).toHaveBeenCalledOnce();
    expect(other).not.toHaveBeenCalled();
  });

  it("does nothing when the front tab is a terminal", () => {
    workspace.layout = tree.createLayout([agent, editor], SESSION);
    const save = vi.fn();
    registerEditorResource(EDITOR_KEY, { save });

    expect(saveActiveEditorResource()).toBe(false);
    expect(save).not.toHaveBeenCalled();
  });

  it("does nothing once the editor tab has unmounted", () => {
    workspace.layout = tree.createLayout([agent, editor], EDITOR_KEY);
    const save = vi.fn();
    registerEditorResource(EDITOR_KEY, { save });
    unregisterEditorResource(EDITOR_KEY);

    expect(saveActiveEditorResource()).toBe(false);
    expect(save).not.toHaveBeenCalled();
  });
});
