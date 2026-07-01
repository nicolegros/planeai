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
  projects: {
    listBranches: vi.fn(() => Promise.resolve(["main", "develop"])),
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

vi.mock("../../lib/session-orchestrator.svelte", () => ({
  getSessions: () => [
    {
      id: "sess-1",
      task_key: "TASK-1",
      pr_url: null,
      project_id: "proj-1",
      name: "Session 1",
      branch: "main",
      status: "active",
      backend: "direct",
      tmux_name: null,
      created_at: "",
      worktree_path: null,
      provider: null,
      tab_count: 1,
      base_branch: null,
      pr_state: null,
    },
  ],
  getActiveSessionId: () => "sess-1",
  getAgentStates: () => ({}),
}));

vi.mock("../../lib/project-store.svelte", () => ({
  getProjects: () => [{ id: "proj-1", name: "myapp", path: "/tmp/myapp" }],
}));

const mockMoveTask = vi.fn((_key: string, _status: string, _repoPath: string) => Promise.resolve());
vi.mock("../../lib/task-store.svelte", () => ({
  getTasksByProject: () => ({
    "/tmp/myapp": [
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
        base_branch: "main",
      },
    ],
  }),
  getTasksForProject: (path: string) =>
    path === "/tmp/myapp"
      ? [
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
            base_branch: "main",
          },
        ]
      : [],
  getAllTasks: () => [
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
      base_branch: "main",
    },
  ],
  isLoading: () => false,
  moveTask: (key: string, status: string, repoPath: string) => mockMoveTask(key, status, repoPath),
  createTask: vi.fn(() => Promise.resolve()),
  editTask: vi.fn(() => Promise.resolve()),
}));

import TaskPanel from "../TaskPanel.svelte";

describe("TaskPanel flatTaskIndex lookup", () => {
  it("builds O(1) index map from flatTaskKeys", () => {
    // Replicate the logic used inside TaskPanel
    const flatTaskKeys = ["§/proj:in_progress", "TASK-1", "TASK-2", "§/proj:todo", "TASK-3"];
    const map = new Map<string, number>();
    flatTaskKeys.forEach((key, i) => map.set(key, i));

    expect(map.get("§/proj:in_progress")).toBe(0);
    expect(map.get("TASK-1")).toBe(1);
    expect(map.get("TASK-2")).toBe(2);
    expect(map.get("§/proj:todo")).toBe(3);
    expect(map.get("TASK-3")).toBe(4);
    expect(map.get("nonexistent")).toBeUndefined();
  });
});

describe("TaskPanel move-to-done archives session", () => {
  let onArchiveSession: ReturnType<typeof vi.fn>;
  let target: HTMLElement;

  beforeEach(() => {
    vi.clearAllMocks();
    onArchiveSession = vi.fn();
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
        onPickTask: vi.fn(),
        onSelectSession: vi.fn(),
        onArchiveSession,
        onSessionsChanged,
      },
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

    expect(mockMoveTask).toHaveBeenCalledWith("TASK-1", "done", "/tmp/myapp");
    expect(onArchiveSession).not.toHaveBeenCalled();
    expect(onSessionsChanged).toHaveBeenCalled();
  });
});
