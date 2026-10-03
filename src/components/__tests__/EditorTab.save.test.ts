import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const { mockGit } = vi.hoisted(() => ({
  mockGit: { readFile: vi.fn(), writeFile: vi.fn() },
}));

vi.mock("../../lib/api", () => ({ git: mockGit }));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    terminal: { font_size: 14, font_family: "monospace" },
    vim_mode: false,
  }),
}));
vi.mock("../../lib/session-orchestrator.svelte", () => ({ recordUserInput: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
const workspace = vi.hoisted(() => ({ layout: null as Layout | null }));
vi.mock("../../lib/task-workspace-layout.svelte", async () => {
  const { focusedTabOf } = await import("../../lib/layout-tree");
  return { taskWorkspaceLayout: { focusedTab: () => focusedTabOf(workspace.layout) } };
});
vi.mock("../../lib/vim-registry", () => ({
  registerEditor: vi.fn(),
  unregisterEditor: vi.fn(),
}));

import { createLayout, focusTab, type Layout } from "../../lib/layout-tree";
import { _resetEditorResources, saveActiveEditorResource } from "../../lib/editor-resources";
import EditorTab from "../EditorTab.svelte";

const SESSION = "session-1";
const FILE = "src/example.ts";
const PTY_KEY = `${SESSION}:editor:${FILE}`;

describe("EditorTab save command wiring", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    vi.clearAllMocks();
    _resetEditorResources();
    mockGit.readFile.mockResolvedValue("const answer = 42;\n");
    mockGit.writeFile.mockResolvedValue(undefined);

    workspace.layout = createLayout(
      [
        { ptyKey: SESSION, label: "Agent", icon: "bot", type: "agent" },
        { ptyKey: PTY_KEY, label: "example.ts", icon: "file", type: "editor", filePath: FILE },
      ],
      PTY_KEY,
    );

    target = document.createElement("div");
    document.body.append(target);
    component = mount(EditorTab, {
      target,
      props: {
        repoPath: "/repo",
        sessionId: SESSION,
        ptyKey: PTY_KEY,
        visible: true,
        focused: true,
        initialFile: FILE,
        onClose: vi.fn(),
        onFocusEditor: vi.fn(),
        onSend: vi.fn(),
      },
    });
  });

  afterEach(() => {
    if (component) unmount(component);
    component = null;
    target.remove();
    workspace.layout = null;
  });

  it("saves the front editor tab through the workspace layout", async () => {
    await tick();
    await vi.waitFor(() => expect(mockGit.readFile).toHaveBeenCalled());

    expect(saveActiveEditorResource()).toBe(true);
    await vi.waitFor(() =>
      expect(mockGit.writeFile).toHaveBeenCalledWith(`/repo/${FILE}`, expect.any(String), "/repo"),
    );
  });

  it("does not save when a terminal tab is in front", async () => {
    await tick();
    await vi.waitFor(() => expect(mockGit.readFile).toHaveBeenCalled());

    workspace.layout = focusTab(workspace.layout!, SESSION);

    expect(saveActiveEditorResource()).toBe(false);
    expect(mockGit.writeFile).not.toHaveBeenCalled();
  });

  it("stops being reachable once the tab unmounts", async () => {
    await tick();
    await vi.waitFor(() => expect(mockGit.readFile).toHaveBeenCalled());

    unmount(component!);
    component = null;

    expect(saveActiveEditorResource()).toBe(false);
    expect(mockGit.writeFile).not.toHaveBeenCalled();
  });
});
