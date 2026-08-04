import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushSync, tick } from "svelte";

const mockCreateTask = vi.fn((_params?: unknown) =>
  Promise.resolve({
    key: "TASK-2",
    title: "New task",
    status: "todo",
    description: "Task description",
    priority: 1,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
  }),
);
const mockMoveTask = vi.fn((_key?: unknown, _status?: unknown, _repoPath?: unknown) => Promise.resolve());
const mockLaunch = vi.fn((_params?: unknown) =>
  Promise.resolve({
    id: "sess-new",
    project_id: "proj-1",
    name: "TASK-2: New task",
    tmux_name: null,
    branch: "task-2/new-task",
    status: "active",
    created_at: "",
    worktree_path: null,
    provider: "claude",
    backend: "direct",
    tab_count: 1,
    base_branch: "main",
    task_key: "TASK-2",
    pr_url: null,
    pr_state: null,
  }),
);

vi.mock("../../lib/api", () => ({
  sessions: { launch: (...args: unknown[]) => mockLaunch(args[0]) },
  projects: { listBranches: vi.fn(() => Promise.resolve(["main", "develop"])) },
  tasks: {
    listAll: vi.fn(() => Promise.resolve([])),
    list: vi.fn(() => Promise.resolve([])),
    create: (params: unknown) => mockCreateTask(params),
    edit: vi.fn(() => Promise.resolve()),
    move: vi.fn(() => Promise.resolve()),
  },
}));

vi.mock("../../lib/snackbar.svelte", () => ({
  showSnackbar: vi.fn(),
}));

vi.mock("../../lib/keyboard", () => ({
  isPlatformMod: () => false,
  MOD_ENTER_HINT: "⌘↵",
}));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({
    providers: { claude: { command: "claude" }, copilot: { command: "gh copilot" } },
    default_provider: "claude",
    task_management: {},
  }),
}));

vi.mock("../../lib/task-store.svelte", () => ({
  getTasksByProject: () => ({}),
  getTasksForProject: () => [],
  getAllTasks: () => [],
  isLoading: () => false,
  createTask: (params: unknown) => mockCreateTask(params),
  moveTask: (key: unknown, status: unknown, repoPath: unknown) => mockMoveTask(key, status, repoPath),
  editTask: vi.fn(() => Promise.resolve()),
}));

import TaskForm from "../TaskForm.svelte";

const baseProps = {
  mode: "create" as const,
  projects: [{ id: "proj-1", name: "My Project", path: "/tmp/myapp" }],
  tasks: [],
  sessions: [],
  onSubmitted: vi.fn(),
  onCancel: vi.fn(),
  onSessionCreated: vi.fn(),
};

function renderForm(props = {}) {
  const target = document.createElement("div");
  mount(TaskForm, { target, props: { ...baseProps, ...props } });
  return target;
}

describe("TaskForm - Start session toggle", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the 'Start session immediately' toggle in create mode", () => {
    const target = renderForm();
    const checkbox = target.querySelector("#start-session") as HTMLInputElement;
    expect(checkbox).not.toBeNull();
  });

  it("toggle defaults to ON in create mode", () => {
    const target = renderForm();
    const checkbox = target.querySelector("#start-session") as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  it("does not show the toggle in edit mode", () => {
    const target = renderForm({ mode: "edit", initial: { key: "TASK-1", title: "Existing" } });
    const checkbox = target.querySelector("#start-session");
    expect(checkbox).toBeNull();
  });

  it("shows session fields when toggle is ON", () => {
    const target = renderForm();
    const sessionBranchField = target.querySelector("[data-field='session-branch']");
    const sessionPromptField = target.querySelector("[data-field='session-prompt']");
    expect(sessionBranchField).not.toBeNull();
    expect(sessionPromptField).not.toBeNull();
  });

  it("hides session fields when toggle is OFF", async () => {
    const target = renderForm();
    const checkbox = target.querySelector("#start-session") as HTMLInputElement;
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
    await tick();
    flushSync();

    const sessionBranchField = target.querySelector("[data-field='session-branch']");
    const sessionPromptField = target.querySelector("[data-field='session-prompt']");
    expect(sessionBranchField).toBeNull();
    expect(sessionPromptField).toBeNull();
  });

  it("submit button says 'Create & Start' when toggle is ON", () => {
    const target = renderForm();
    const submitBtn = target.querySelector("button[type='submit']")!;
    expect(submitBtn.textContent).toContain("Create & Start");
  });

  it("submit button says 'Create' when toggle is OFF", async () => {
    const target = renderForm();
    const checkbox = target.querySelector("#start-session") as HTMLInputElement;
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
    await tick();
    flushSync();

    const submitBtn = target.querySelector("button[type='submit']")!;
    expect(submitBtn.textContent).toContain("Create");
    expect(submitBtn.textContent).not.toContain("Create & Start");
  });

  it("shows provider select when multiple providers are configured", () => {
    const target = renderForm();
    // Should see "Provider" label since we have claude + copilot
    expect(target.textContent).toContain("Provider");
  });

  it("auto-generates branch name from title", async () => {
    const target = renderForm();
    const titleInput = target.querySelector("[data-field='title'] input") as HTMLInputElement;
    titleInput.value = "Fix login redirect";
    titleInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // The branch field should show the placeholder or preview text
    const branchInput = target.querySelector("[data-field='session-branch'] input") as HTMLInputElement;
    expect(branchInput.placeholder).toContain("fix-login-redirect");
  });

  it("creates task and launches session on submit with toggle ON", async () => {
    const onSessionCreated = vi.fn();
    const onSubmitted = vi.fn();
    const target = renderForm({ onSessionCreated, onSubmitted });

    // Fill title
    const titleInput = target.querySelector("[data-field='title'] input") as HTMLInputElement;
    titleInput.value = "New task";
    titleInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Submit
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    // Wait for async operations
    await new Promise((r) => setTimeout(r, 50));

    expect(mockCreateTask).toHaveBeenCalled();
    expect(mockMoveTask).toHaveBeenCalledWith("TASK-2", "in_progress", "/tmp/myapp");
    expect(mockLaunch).toHaveBeenCalled();
    expect(onSessionCreated).toHaveBeenCalled();
  });

  it("creates task without session when toggle is OFF", async () => {
    const onSessionCreated = vi.fn();
    const onSubmitted = vi.fn();
    const target = renderForm({ onSessionCreated, onSubmitted });

    // Turn off toggle
    const checkbox = target.querySelector("#start-session") as HTMLInputElement;
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
    await tick();
    flushSync();

    // Fill title
    const titleInput = target.querySelector("[data-field='title'] input") as HTMLInputElement;
    titleInput.value = "Backlog task";
    titleInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Submit
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    await new Promise((r) => setTimeout(r, 50));

    expect(mockCreateTask).toHaveBeenCalled();
    expect(mockMoveTask).not.toHaveBeenCalled();
    expect(mockLaunch).not.toHaveBeenCalled();
    expect(onSessionCreated).not.toHaveBeenCalled();
    expect(onSubmitted).toHaveBeenCalled();
  });

  it("keeps task and shows error if session launch fails", async () => {
    const { showSnackbar } = await import("../../lib/snackbar.svelte");
    mockLaunch.mockRejectedValueOnce(new Error("Agent not found"));
    const onSessionCreated = vi.fn();
    const onSubmitted = vi.fn();
    const target = renderForm({ onSessionCreated, onSubmitted });

    // Fill title
    const titleInput = target.querySelector("[data-field='title'] input") as HTMLInputElement;
    titleInput.value = "Task with bad session";
    titleInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Submit
    const form = target.querySelector("form")!;
    form.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    await new Promise((r) => setTimeout(r, 50));

    // Task should still be created
    expect(mockCreateTask).toHaveBeenCalled();
    // Session creation attempted
    expect(mockLaunch).toHaveBeenCalled();
    // But session created callback not called
    expect(onSessionCreated).not.toHaveBeenCalled();
    // Snackbar shown with error
    expect(showSnackbar).toHaveBeenCalledWith(expect.stringContaining("session failed"));
    // onSubmitted still called (task was created)
    expect(onSubmitted).toHaveBeenCalled();
  });
});
