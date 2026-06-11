import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mount, flushSync } from "svelte";

const mockInvoke = vi.fn(() => Promise.resolve([]));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: any[]) => mockInvoke(...args),
}));

vi.mock("../../lib/snackbar.svelte", () => ({
  showSnackbar: vi.fn(),
}));

vi.mock("../../lib/keyboard", () => ({
  isPlatformMod: () => false,
  MOD_ENTER_HINT: "⌘↵",
}));

vi.mock("../../lib/focus.svelte", () => ({
  focusTerminal: vi.fn(),
}));

import TaskPanel from "../TaskPanel.svelte";

describe("TaskPanel move-to-done archives session", () => {
  let onArchiveSession: ReturnType<typeof vi.fn>;
  let target: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    onArchiveSession = vi.fn();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_all_task_items") {
        return Promise.resolve([
          { key: "TASK-1", title: "Fix bug", status: "in_progress", description: "", priority: 1, blocked_by: [], tags: [], url: null },
        ]);
      }
      if (cmd === "move_task_item") return Promise.resolve();
      return Promise.resolve([]);
    });

    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    document.body.removeChild(target);
  });

  it("calls onArchiveSession when context menu → Done is clicked for a task with linked session", async () => {
    mount(TaskPanel, {
      target,
      props: {
        projects: [{ name: "myapp", path: "/tmp/myapp" }],
        sessions: [{ id: "sess-1", task_key: "TASK-1" }],
        agentStates: {},
        onPickTask: vi.fn(),
        onSelectSession: vi.fn(),
        onArchiveSession,
        onTaskCreateConsumed: vi.fn(),
      },
    });

    // Wait for tasks to load
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("list_all_task_items", { repoPath: "/tmp/myapp" });
    });
    await new Promise((r) => setTimeout(r, 10));

    // Find the task item button and right-click it
    const taskButtons = target.querySelectorAll("button");
    const taskBtn = Array.from(taskButtons).find((b) => b.textContent?.includes("TASK-1"));
    expect(taskBtn).toBeDefined();

    taskBtn!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 50, clientY: 50 }));
    await new Promise((r) => setTimeout(r, 10));

    // Find the Done menu item in the whole document (context menu may portal)
    const allButtons = document.querySelectorAll("button");
    const doneBtn = Array.from(allButtons).find((b) => b.textContent?.trim() === "→ Done");
    expect(doneBtn).toBeDefined();

    doneBtn!.click();
    await new Promise((r) => setTimeout(r, 10));

    expect(mockInvoke).toHaveBeenCalledWith("move_task_item", {
      key: "TASK-1",
      status: "done",
      repoPath: "/tmp/myapp",
    });
    expect(onArchiveSession).toHaveBeenCalledWith({ id: "sess-1", task_key: "TASK-1" });
  });

  it("does NOT call onArchiveSession when no session linked to task", async () => {
    mount(TaskPanel, {
      target,
      props: {
        projects: [{ name: "myapp", path: "/tmp/myapp" }],
        sessions: [{ id: "sess-1", task_key: "OTHER" }],
        agentStates: {},
        onPickTask: vi.fn(),
        onSelectSession: vi.fn(),
        onArchiveSession,
        onTaskCreateConsumed: vi.fn(),
      },
    });

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("list_all_task_items", { repoPath: "/tmp/myapp" });
    });
    await new Promise((r) => setTimeout(r, 10));

    const taskButtons = target.querySelectorAll("button");
    const taskBtn = Array.from(taskButtons).find((b) => b.textContent?.includes("TASK-1"));
    expect(taskBtn).toBeDefined();

    taskBtn!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 50, clientY: 50 }));
    await new Promise((r) => setTimeout(r, 10));

    const allButtons = document.querySelectorAll("button");
    const doneBtn = Array.from(allButtons).find((b) => b.textContent?.trim() === "→ Done");
    expect(doneBtn).toBeDefined();

    doneBtn!.click();
    await new Promise((r) => setTimeout(r, 10));

    expect(mockInvoke).toHaveBeenCalledWith("move_task_item", {
      key: "TASK-1",
      status: "done",
      repoPath: "/tmp/myapp",
    });
    expect(onArchiveSession).not.toHaveBeenCalled();
  });
});
