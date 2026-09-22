import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const { mockGit, mockPty } = vi.hoisted(() => ({
  mockGit: { readFile: vi.fn(), writeFile: vi.fn() },
  mockPty: { write: vi.fn() },
}));

vi.mock("../../lib/api", () => ({ git: mockGit, pty: mockPty }));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    terminal: { font_size: 14, font_family: "monospace" },
    vim_mode: false,
  }),
}));
vi.mock("../../lib/session-orchestrator.svelte", () => ({ recordUserInput: vi.fn() }));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../../lib/vim-registry", () => ({
  registerEditor: vi.fn(),
  unregisterEditor: vi.fn(),
}));

import * as splitTree from "../../lib/split-tree.svelte";
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

    splitTree.resetTree();
    splitTree.initTree(
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
      },
    });
  });

  afterEach(() => {
    if (component) unmount(component);
    component = null;
    target.remove();
    splitTree.resetTree();
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

    splitTree.focusTab(SESSION);

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
