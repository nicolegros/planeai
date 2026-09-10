import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

const { mockGit, mockPty, mockRecordUserInput, mockShowSnackbar } = vi.hoisted(() => ({
  mockGit: { readFile: vi.fn() },
  mockPty: { write: vi.fn() },
  mockRecordUserInput: vi.fn(),
  mockShowSnackbar: vi.fn(),
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
vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: mockShowSnackbar }));
vi.mock("../../lib/vim-registry", () => ({
  registerEditor: vi.fn(),
  unregisterEditor: vi.fn(),
}));

import {
  _resetForTests as resetEditorFeedback,
  addEditorFeedback,
  getEditorFeedbackCount,
} from "../../lib/editor-feedback.svelte";
import EditorTab from "../EditorTab.svelte";

describe("EditorTab feedback send shortcut", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount>;

  beforeEach(() => {
    resetEditorFeedback();
    mockGit.readFile.mockResolvedValue("const answer = 42;\n");
    mockPty.write.mockResolvedValue(true);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(EditorTab, {
      target,
      props: {
        repoPath: "/repo",
        sessionId: "session-1",
        visible: true,
        focused: true,
        initialFile: "src/example.ts",
        onClose: vi.fn(),
        onFocusEditor: vi.fn(),
      },
    });
  });

  afterEach(() => {
    unmount(component);
    target.remove();
    resetEditorFeedback();
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

    editor.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    await vi.waitFor(() => expect(mockPty.write).toHaveBeenCalledOnce());
    expect(mockPty.write).toHaveBeenCalledWith("session-1", expect.any(Array));
    const sentBytes = mockPty.write.mock.calls[0][1] as number[];
    expect(sentBytes.at(-1)).toBe(0x0d);
  });

  it("does not intercept the shortcut from a form control", async () => {
    addEditorFeedback("session-1", {
      filePath: "src/example.ts",
      startLine: 1,
      endLine: 1,
      language: "typescript",
      selectedText: "const answer = 42;",
      contextBefore: "",
      contextAfter: "",
      isUnsaved: false,
      text: "Keep this form shortcut local.",
    });
    const formControl = document.createElement("textarea");
    document.body.append(formControl);

    formControl.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await tick();

    expect(mockPty.write).not.toHaveBeenCalled();
    formControl.remove();
  });

  it("does not let an unfocused visible editor own the shortcut", async () => {
    const inactiveTarget = document.createElement("div");
    document.body.append(inactiveTarget);
    const inactiveEditor = mount(EditorTab, {
      target: inactiveTarget,
      props: {
        repoPath: "/repo",
        sessionId: "session-2",
        visible: true,
        focused: false,
        onClose: vi.fn(),
        onFocusEditor: vi.fn(),
      },
    });
    addEditorFeedback("session-2", {
      filePath: "src/example.ts",
      startLine: 1,
      endLine: 1,
      language: "typescript",
      selectedText: "const answer = 42;",
      contextBefore: "",
      contextAfter: "",
      isUnsaved: false,
      text: "Do not send from an inactive editor.",
    });

    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await tick();

    expect(mockPty.write).not.toHaveBeenCalled();
    unmount(inactiveEditor);
    inactiveTarget.remove();
  });

  it("preserves queued feedback and retries after a write failure", async () => {
    addEditorFeedback("session-1", {
      filePath: "src/example.ts",
      startLine: 1,
      endLine: 1,
      language: "typescript",
      selectedText: "const answer = 42;",
      contextBefore: "",
      contextAfter: "",
      isUnsaved: false,
      text: "Retry this feedback.",
    });
    mockPty.write.mockRejectedValueOnce(new Error("PTY unavailable"));

    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(mockPty.write).toHaveBeenCalledOnce());
    await tick();

    expect(getEditorFeedbackCount("session-1")).toBe(1);
    mockPty.write.mockResolvedValueOnce(true);
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(mockPty.write).toHaveBeenCalledTimes(2));

    await vi.waitFor(() => expect(getEditorFeedbackCount("session-1")).toBe(0));
    expect((mockPty.write.mock.calls[1][1] as number[]).at(-1)).toBe(0x0d);
  });

  it("retains feedback when the PTY transport does not acknowledge delivery", async () => {
    addEditorFeedback("session-1", {
      filePath: "src/example.ts",
      startLine: 1,
      endLine: 1,
      language: "typescript",
      selectedText: "const answer = 42;",
      contextBefore: "",
      contextAfter: "",
      isUnsaved: false,
      text: "Do not lose this feedback.",
    });
    mockPty.write.mockResolvedValueOnce(false);

    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await vi.waitFor(() => expect(mockPty.write).toHaveBeenCalledOnce());

    await vi.waitFor(() => expect(mockShowSnackbar).toHaveBeenCalled());
    expect(getEditorFeedbackCount("session-1")).toBe(1);
    expect(mockShowSnackbar).toHaveBeenCalledWith(
      "PTY session is not attached. Try again once it is ready.",
      "error",
    );
  });

  it("preserves a modified editor-feedback draft when switching notes is cancelled", async () => {
    for (const text of ["First queued note.", "Second queued note."]) {
      addEditorFeedback("session-1", {
        filePath: "src/example.ts",
        startLine: 1,
        endLine: 1,
        language: "typescript",
        selectedText: "const answer = 42;",
        contextBefore: "",
        contextAfter: "",
        isUnsaved: false,
        text,
      });
    }
    await tick();

    Array.from(target.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.includes("Pending editor feedback"))
      ?.click();
    await tick();
    const editButtons = target.querySelectorAll<HTMLButtonElement>(
      "[aria-label='Edit editor feedback']",
    );
    editButtons[0]?.click();
    await tick();

    const textarea = target.querySelector<HTMLTextAreaElement>(
      "textarea[aria-label='Editor feedback comment']",
    );
    textarea!.value = "Unsaved first-note draft.";
    textarea!.dispatchEvent(new Event("input", { bubbles: true }));
    vi.spyOn(window, "confirm").mockReturnValue(false);
    editButtons[1]?.click();
    await tick();

    expect(window.confirm).toHaveBeenCalledWith("Discard the unsaved editor feedback?");
    expect(textarea!.value).toBe("Unsaved first-note draft.");
  });

  it("keeps a typed draft when Escape discard confirmation is rejected", async () => {
    const editor = await vi.waitFor(() => {
      const content = target.querySelector<HTMLElement>(".cm-content");
      expect(content).toBeTruthy();
      return content!;
    });
    editor.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "a",
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    await tick();
    Array.from(target.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent === "Comment")
      ?.click();
    await tick();

    const textarea = target.querySelector<HTMLTextAreaElement>(
      "textarea[aria-label='Editor feedback comment']",
    );
    textarea!.value = "Keep this draft.";
    textarea!.dispatchEvent(new Event("input", { bubbles: true }));
    vi.spyOn(window, "confirm").mockReturnValue(false);
    textarea!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    await tick();

    expect(window.confirm).toHaveBeenCalledWith("Discard the unsaved editor feedback?");
    expect(textarea!.value).toBe("Keep this draft.");
  });
});
