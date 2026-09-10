import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const { mockGit, mockPty, mockRecordUserInput } = vi.hoisted(() => ({
  mockGit: { readFile: vi.fn() },
  mockPty: { write: vi.fn() },
  mockRecordUserInput: vi.fn(),
}));

vi.mock("../../lib/api", () => ({
  git: mockGit,
  pty: mockPty,
}));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    terminal: { font_size: 14, font_family: "monospace" },
    vim_mode: false,
  }),
}));
vi.mock("../../lib/session-orchestrator.svelte", () => ({
  recordUserInput: mockRecordUserInput,
}));
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../../lib/vim-registry", () => ({
  registerEditor: vi.fn(),
  unregisterEditor: vi.fn(),
}));

import EditorTab from "../EditorTab.svelte";

describe("EditorTab feedback send shortcut", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    mockGit.readFile.mockResolvedValue("const answer = 42;\n");
    mockPty.write.mockResolvedValue(undefined);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(EditorTab, {
      target,
      props: {
        repoPath: "/repo",
        sessionId: "session-1",
        visible: true,
        initialFile: "src/example.ts",
        onClose: vi.fn(),
        onFocusEditor: vi.fn(),
      },
    });
  });

  afterEach(() => {
    unmount(component);
    target.remove();
    vi.clearAllMocks();
  });

  it("sends pending feedback with the platform modifier after Enter queues a note", async () => {
    const editor = await vi.waitFor(() => {
      const content = target.querySelector<HTMLElement>(".cm-content");
      expect(content).toBeTruthy();
      return content!;
    });

    editor.focus();
    editor.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "a",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await tick();

    const commentButton = Array.from(target.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "Comment",
    );
    expect(commentButton).toBeTruthy();
    expect(commentButton?.disabled).toBe(false);
    commentButton?.click();
    await tick();

    const textarea = target.querySelector<HTMLTextAreaElement>(
      "textarea[aria-label='Editor feedback comment']",
    );
    expect(textarea).toBeTruthy();
    textarea!.value = "Send this to the agent.";
    textarea!.dispatchEvent(new Event("input", { bubbles: true }));
    textarea!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );
    await tick();

    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => expect(mockPty.write).toHaveBeenCalledTimes(2));
    expect(mockPty.write).toHaveBeenNthCalledWith(1, "session-1", expect.any(Array));
    expect(mockPty.write).toHaveBeenNthCalledWith(2, "session-1", [0x0d]);
  });
});
