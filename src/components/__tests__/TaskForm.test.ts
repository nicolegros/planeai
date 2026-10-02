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
const mockMoveTask = vi.fn((_key?: unknown, _status?: unknown, _repoPath?: unknown) =>
  Promise.resolve(),
);
const mockLaunch = vi.fn((_params?: unknown) =>
  Promise.resolve({
    session: {
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
      base_branch: "main",
      task_key: "TASK-2",
    },
    warning: null,
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
  moveTask: (key: unknown, status: unknown, repoPath: unknown) =>
    mockMoveTask(key, status, repoPath),
  editTask: vi.fn(() => Promise.resolve()),
}));

import TaskForm from "../TaskForm.svelte";

const baseProps = {
  mode: "create" as const,
  projects: [{ id: "proj-1", name: "My Project", path: "/tmp/myapp", hidden: false }],
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
    const branchInput = target.querySelector(
      "[data-field='session-branch'] input",
    ) as HTMLInputElement;
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

describe("TaskForm - Random default title", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function titleInput(target: HTMLElement) {
    return target.querySelector("[data-field='title'] input") as HTMLInputElement;
  }

  it("prefills a kebab-case random title in create mode", () => {
    const target = renderForm();
    expect(titleInput(target).value).toMatch(/^[a-z]+-[a-z]+$/);
  });

  it("keeps the provided initial title", () => {
    const target = renderForm({ initial: { title: "Given title" } });
    expect(titleInput(target).value).toBe("Given title");
  });

  it("does not generate a title in edit mode", () => {
    const target = renderForm({ mode: "edit", initial: { key: "TASK-1" } });
    expect(titleInput(target).value).toBe("");
  });

  it("enables submit with the generated title", () => {
    const target = renderForm();
    const submitBtn = target.querySelector("button[type='submit']") as HTMLButtonElement;
    expect(submitBtn.disabled).toBe(false);
  });

  it("selects the generated title on focus", () => {
    const target = renderForm();
    document.body.appendChild(target);
    const input = titleInput(target);
    input.focus();
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);
    target.remove();
  });

  it("keeps the selection when focusing the generated title with a click", () => {
    const target = renderForm();
    document.body.appendChild(target);
    const input = titleInput(target);
    input.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    input.focus();
    const mouseup = new MouseEvent("mouseup", { bubbles: true, cancelable: true });
    input.dispatchEvent(mouseup);
    expect(mouseup.defaultPrevented).toBe(true);

    const nextMouseup = new MouseEvent("mouseup", { bubbles: true, cancelable: true });
    input.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    input.dispatchEvent(nextMouseup);
    expect(nextMouseup.defaultPrevented).toBe(false);
    target.remove();
  });

  it("generates a title when the initial title is empty", () => {
    const target = renderForm({ initial: { title: "" } });
    expect(titleInput(target).value).toMatch(/^[a-z]+-[a-z]+$/);
  });

  it("does not select the title on focus once edited", () => {
    const target = renderForm();
    document.body.appendChild(target);
    const input = titleInput(target);
    input.value = "My own title";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    input.setSelectionRange(input.value.length, input.value.length);
    input.blur();
    input.focus();
    expect(input.selectionStart).toBe(input.value.length);
    target.remove();
  });

  it("shows a note describing the generated title", () => {
    const target = renderForm();
    const input = titleInput(target);
    const note = target.querySelector(`#${input.getAttribute("aria-describedby")}`);
    expect(note?.textContent).toContain("Randomly generated");
  });

  it("hides the note once the title is edited", () => {
    const target = renderForm();
    const input = titleInput(target);
    input.value = "My own title";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(target.textContent).not.toContain("Randomly generated");
    expect(input.hasAttribute("aria-describedby")).toBe(false);
  });

  it("does not show the note for a provided title", () => {
    const target = renderForm({ initial: { title: "Given title" } });
    expect(target.textContent).not.toContain("Randomly generated");
  });

  it("submits the generated title when left untouched", async () => {
    const target = renderForm();
    const generated = titleInput(target).value;
    target.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    await new Promise((r) => setTimeout(r, 50));
    expect(mockCreateTask).toHaveBeenCalledWith(expect.objectContaining({ title: generated }));
  });
});
