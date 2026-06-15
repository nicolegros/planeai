import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mount } from "svelte";

vi.mock("../../lib/api", () => ({
  tasks: {
    listAll: vi.fn(() => Promise.resolve([])),
    list: vi.fn(() => Promise.resolve([])),
    move: vi.fn(() => Promise.resolve()),
    create: vi.fn(() => Promise.resolve()),
    edit: vi.fn(() => Promise.resolve()),
  },
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
  getActiveZone: () => "terminal",
  getSidebarSubZone: () => "sessions",
}));

vi.mock("../../lib/sidebar-nav.svelte", () => ({
  getSelectedIndex: () => 0,
  setSelectedIndex: vi.fn(),
  clampIndex: vi.fn(),
  handleSidebarKey: () => null,
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({ hide_done_tasks: false }),
  updateSettings: vi.fn(),
}));

import { tasks } from "../../lib/api";
import TaskPanel from "../TaskPanel.svelte";

describe("TaskPanel move-to-done archives session", () => {
  let onArchiveSession: ReturnType<typeof vi.fn>;
  let target: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    onArchiveSession = vi.fn();
    vi.mocked(tasks.listAll).mockResolvedValue([
      {
        key: "TASK-1",
        title: "Fix bug",
        status: "in_progress",
        description: "",
        priority: 1,
        blocked_by: [],
        tags: [],
        parent_key: null,
        url: null,
      },
    ]);

    target = document.createElement("div");
    document.body.appendChild(target);
  });

  afterEach(() => {
    document.body.removeChild(target);
  });

  it("calls onSessionsChanged when task status changes via context menu", async () => {
    const onSessionsChanged = vi.fn();
    mount(TaskPanel, {
      target,
      props: {
        projects: [{ name: "myapp", path: "/tmp/myapp" }],
        sessions: [{ id: "sess-1", task_key: "TASK-1", pr_url: null }],
        agentStates: {},
        onPickTask: vi.fn(),
        onSelectSession: vi.fn(),
        onArchiveSession,
        onSessionsChanged,
        onTaskCreateConsumed: vi.fn(),
      },
    });

    await vi.waitFor(() => {
      expect(tasks.listAll).toHaveBeenCalledWith("/tmp/myapp");
    });
    await new Promise((r) => setTimeout(r, 10));

    const taskButtons = target.querySelectorAll("button");
    const taskBtn = Array.from(taskButtons).find((b) => b.textContent?.includes("TASK-1"));
    expect(taskBtn).toBeDefined();

    taskBtn!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, clientX: 50, clientY: 50 }),
    );
    await new Promise((r) => setTimeout(r, 10));

    const allButtons = document.querySelectorAll("button");
    const doneBtn = Array.from(allButtons).find((b) => b.textContent?.trim() === "→ Done");
    expect(doneBtn).toBeDefined();

    doneBtn!.click();
    await new Promise((r) => setTimeout(r, 10));

    expect(tasks.move).toHaveBeenCalledWith("TASK-1", "done", "/tmp/myapp");
    expect(onArchiveSession).not.toHaveBeenCalled();
    expect(onSessionsChanged).toHaveBeenCalled();
  });
});
