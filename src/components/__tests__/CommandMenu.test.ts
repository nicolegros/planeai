import { afterEach, describe, expect, it, vi } from "vitest";
import { mount, tick, unmount } from "svelte";

class ResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

vi.stubGlobal("ResizeObserver", ResizeObserver);
Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value: vi.fn(),
});

function makeSession(overrides: Record<string, unknown> = {}) {
  return {
    id: "session-1",
    project_id: "project-1",
    name: "Existing session",
    branch: "main",
    status: "active",
    backend: "direct",
    tmux_name: null,
    created_at: "",
    worktree_path: null,
    provider: null,
    tab_count: 1,
    base_branch: null,
    task_key: null,
    ...overrides,
  };
}

function makeTask(overrides: Record<string, unknown> = {}) {
  return {
    key: "PLA-1",
    title: "Task one",
    status: "todo",
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
    ...overrides,
  };
}

let sessions: ReturnType<typeof makeSession>[] = [makeSession()];
let tasksByProject: Record<string, ReturnType<typeof makeTask>[]> = {};
let activeSessionId: string | null = "session-1";
let loopSessionIds = new Set<string>();

vi.mock("../../lib/api", () => ({
  sessions: { listArchived: vi.fn(() => Promise.resolve([])), restore: vi.fn(), destroy: vi.fn() },
  projects: {
    getAutoMode: vi.fn(),
    setAutoMode: vi.fn(),
    listArchived: vi.fn(() => Promise.resolve([])),
  },
  git: { listFiles: vi.fn(() => Promise.resolve([])) },
}));

vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({ hide_done_tasks: false, hide_empty_projects: false }),
  updateSettings: vi.fn(),
}));
vi.mock("../../lib/session-orchestrator.svelte", () => ({
  getSessions: () => sessions,
  getActiveSessionId: () => activeSessionId,
}));
vi.mock("../../lib/project-store.svelte", () => ({
  getProjects: () => [{ id: "project-1", name: "Project", path: "/tmp/project" }],
}));
vi.mock("../../lib/task-store.svelte", () => ({
  getTasksByProject: () => tasksByProject,
}));
vi.mock("../../lib/loop-store.svelte", () => ({
  getLoopIdForSession: (id: string) => (loopSessionIds.has(id) ? "loop-1" : null),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import CommandMenu from "../CommandMenu.svelte";

const ROOT_INPUT = "input[placeholder='Go to a task, session, or action…']";

function menuProps() {
  return {
    open: true,
    onOpenChange: vi.fn(),
    onSelectSession: vi.fn(),
    onArchiveSession: vi.fn(),
    onDeleteSession: vi.fn(),
    onNewSession: vi.fn(),
    onRenameSession: vi.fn(),
    onRestoreSession: vi.fn(),
    onDestroyArchivedSession: vi.fn(),
    onResetTerminal: vi.fn(),
    onArchiveProject: vi.fn(),
    onHideProject: vi.fn(),
    onUnhideProject: vi.fn(),
    onDeleteProject: vi.fn(),
    onRestoreProject: vi.fn(),
    onSelectTask: vi.fn(),
    onCreateTask: vi.fn(),
    onToggleDiff: vi.fn(),
  };
}

async function findRootInput(): Promise<HTMLInputElement> {
  return await vi.waitFor(() => {
    const next = document.querySelector<HTMLInputElement>(ROOT_INPUT);
    expect(next).toBeTruthy();
    return next!;
  });
}

async function typeQuery(input: HTMLInputElement, text: string): Promise<void> {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
  await tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await tick();
}

function visibleItemLabels(): string[] {
  return [...document.querySelectorAll("[data-command-item]")]
    .filter((el) => !el.hasAttribute("hidden") && el.getAttribute("aria-hidden") !== "true")
    .map((el) => el.textContent?.trim() ?? "");
}

describe("CommandMenu", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) unmount(component);
    await new Promise((resolve) => setTimeout(resolve, 30));
    target?.remove();
    sessions = [makeSession()];
    tasksByProject = {};
    activeSessionId = "session-1";
    loopSessionIds = new Set();
  });

  function render(props = menuProps()) {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(CommandMenu, { target, props });
    return props;
  }

  it("makes New session the initial keyboard selection instead of switching to an existing session", async () => {
    const props = render();
    const input = await findRootInput();

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(props.onSelectSession).not.toHaveBeenCalled();
    expect(props.onNewSession).toHaveBeenCalledOnce();
  });

  it("selects a task by title and reports its repo path", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-42", title: "Fix the launcher" })] };
    const props = render();
    const input = await findRootInput();

    await typeQuery(input, "Fix the launcher");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(props.onSelectTask).toHaveBeenCalledOnce();
    const [task, repoPath] = props.onSelectTask.mock.calls[0]!;
    expect(task.key).toBe("PLA-42");
    expect(repoPath).toBe("/tmp/project");
  });

  it("selects a task by key", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-42", title: "Fix the launcher" })] };
    const props = render();
    const input = await findRootInput();

    await typeQuery(input, "PLA-42");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(props.onSelectTask).toHaveBeenCalledOnce();
    expect(props.onSelectTask.mock.calls[0]![0].key).toBe("PLA-42");
  });

  it("finds tasks when there is no active session", async () => {
    activeSessionId = null;
    sessions = [];
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-42", title: "Fix the launcher" })] };
    const props = render();
    const input = await findRootInput();

    await typeQuery(input, "Fix the launcher");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(props.onSelectTask).toHaveBeenCalledOnce();
  });

  it("does not list tasks before anything is typed", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-42", title: "Fix the launcher" })] };
    render();
    const input = await findRootInput();
    await tick();

    // Guard against a vacuous assertion: the selector must actually match items.
    const before = visibleItemLabels();
    expect(before.length).toBeGreaterThan(0);
    expect(before.some((label) => label.includes("PLA-42"))).toBe(false);

    await typeQuery(input, "Fix the launcher");

    expect(visibleItemLabels().some((label) => label.includes("PLA-42"))).toBe(true);
  });

  it("does not match a task by its repo path", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-42", title: "Fix the launcher" })] };
    const props = render();
    const input = await findRootInput();

    await typeQuery(input, "/tmp/project");

    expect(visibleItemLabels().some((label) => label.includes("PLA-42"))).toBe(false);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(props.onSelectTask).not.toHaveBeenCalled();
  });

  it("does not pull focus back to the previously focused element after selecting a task", async () => {
    // The dialog's focus scope restores focus to whatever was focused before it opened, which
    // would undo the focus the app moves to the selected task's terminal.
    const sentinel = document.createElement("input");
    document.body.append(sentinel);
    sentinel.focus();
    expect(document.activeElement).toBe(sentinel);

    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-42", title: "Fix the launcher" })] };
    const props = menuProps();
    const onOpenChange = vi.fn();
    props.onOpenChange = onOpenChange;
    render(props);
    const input = await findRootInput();

    await typeQuery(input, "Fix the launcher");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(props.onSelectTask).toHaveBeenCalledOnce();
    expect(onOpenChange).toHaveBeenCalledWith(false);

    // Tear the dialog down the way the app does when `open` flips to false.
    if (component) unmount(component);
    component = undefined;
    await new Promise((resolve) => setTimeout(resolve, 30));

    expect(document.activeElement).not.toBe(sentinel);
    sentinel.remove();
  });

  it("no longer offers the Pick task submenu", async () => {
    render();
    await findRootInput();
    await tick();

    expect(document.body.textContent).not.toContain("Pick task");
  });

  it("omits a session that a visible task can navigate to", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-1", title: "Task one" })] };
    sessions = [makeSession({ id: "linked", name: "Linked session", task_key: "PLA-1" })];
    render();
    await findRootInput();
    await tick();

    expect(document.body.textContent).not.toContain("Linked session");
  });

  it("keeps listing a session with no task", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-1", title: "Task one" })] };
    sessions = [makeSession({ id: "orphan", name: "Orphan session", task_key: null })];
    render();
    await findRootInput();
    await tick();

    expect(document.body.textContent).toContain("Orphan session");
  });

  it("keeps listing a loop session even when a task could navigate to it", async () => {
    tasksByProject = { "/tmp/project": [makeTask({ key: "PLA-1", title: "Task one" })] };
    sessions = [makeSession({ id: "loop-session", name: "Maker round 1", task_key: "PLA-1" })];
    loopSessionIds = new Set(["loop-session"]);
    render();
    await findRootInput();
    await tick();

    expect(document.body.textContent).toContain("Maker round 1");
  });
});
