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

const sessions = [
  {
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
    pr_url: null,
    pr_state: null,
  },
];

vi.mock("../../lib/api", () => ({
  sessions: { listArchived: vi.fn(() => Promise.resolve([])), restore: vi.fn(), destroy: vi.fn() },
  projects: { getAutoMode: vi.fn(), setAutoMode: vi.fn() },
  tasks: { list: vi.fn(() => Promise.resolve([])) },
  git: { listFiles: vi.fn(() => Promise.resolve([])) },
}));

vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar: vi.fn() }));
vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({ hide_done_tasks: false, hide_empty_projects: false }),
  updateSettings: vi.fn(),
}));
vi.mock("../../lib/session-orchestrator.svelte", () => ({
  getSessions: () => sessions,
  getActiveSessionId: () => "session-1",
}));
vi.mock("../../lib/project-store.svelte", () => ({
  getProjects: () => [{ id: "project-1", name: "Project", path: "/tmp/project" }],
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import CommandMenu from "../CommandMenu.svelte";

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
    onPickTask: vi.fn(),
    onCreateTask: vi.fn(),
    onToggleDiff: vi.fn(),
  };
}

describe("CommandMenu", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) unmount(component);
    await new Promise((resolve) => setTimeout(resolve, 30));
    target?.remove();
  });

  it("makes New session the initial keyboard selection instead of switching to an existing session", async () => {
    const onNewSession = vi.fn();
    const onSelectSession = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    const props = menuProps();
    props.onNewSession = onNewSession;
    props.onSelectSession = onSelectSession;
    component = mount(CommandMenu, {
      target,
      props,
    });

    const input = await vi.waitFor(() => {
      const next = document.querySelector<HTMLInputElement>(
        "input[placeholder='Go to a session, task, or action…']",
      );
      expect(next).toBeTruthy();
      return next!;
    });

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();

    expect(onSelectSession).not.toHaveBeenCalled();
    expect(onNewSession).toHaveBeenCalledOnce();
  });
});
