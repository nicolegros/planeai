import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import type { Project, Session, TaskItem } from "../../lib/types";

const { configGet, configUpdate, projectList, sessionList, showSnackbar, taskListAll, taskMove } =
  vi.hoisted(() => ({
    showSnackbar: vi.fn(),
    configGet: vi.fn(),
    configUpdate: vi.fn(),
    projectList: vi.fn(),
    sessionList: vi.fn(),
    taskListAll: vi.fn(),
    taskMove: vi.fn(),
  }));

vi.mock("../../lib/api", () => ({
  config: { get: configGet, update: configUpdate, refresh: configGet },
  plugins: {
    call: vi.fn(),
    localUiSource: vi.fn(),
    dataChanged: vi.fn().mockResolvedValue(undefined),
  },
  projects: {
    list: projectList,
    getAutoMode: vi.fn().mockResolvedValue(false),
    setAutoMode: vi.fn().mockResolvedValue(undefined),
  },
  sessions: {
    list: sessionList,
    acknowledge: vi.fn().mockResolvedValue(undefined),
    saveMruOrder: vi.fn().mockResolvedValue(undefined),
  },
  tasks: { listAll: taskListAll, move: taskMove },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../lib/snackbar.svelte", () => ({ showSnackbar }));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ onCloseRequested: vi.fn().mockResolvedValue(() => {}) })),
}));

import UnifiedSidebar from "../UnifiedSidebar.svelte";
import { focusSidebar, focusTerminal } from "../../lib/focus.svelte";
import { loadProjects } from "../../lib/project-store.svelte";
import { _resetForTests, loadSessions } from "../../lib/session-orchestrator.svelte";
import { getSelectedIndex, setSelectedIndex } from "../../lib/sidebar-nav.svelte";
import { loadTasks } from "../../lib/task-store.svelte";
import { getSettings, loadSettings } from "../../lib/settings.svelte";

const alpha: Project = { id: "alpha", name: "alpha", path: "/repos/alpha", hidden: false };
const beta: Project = { id: "beta", name: "beta-service", path: "/repos/beta", hidden: false };

function task(key: string, status: string, priority = 0): TaskItem {
  return {
    key,
    title: `Title ${key}`,
    status,
    description: "",
    priority,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
  };
}

function session(id: string, projectId: string): Session {
  return {
    id,
    project_id: projectId,
    name: id,
    tmux_name: null,
    branch: id,
    status: "active",
    created_at: "",
    worktree_path: null,
    provider: null,
    backend: "local",
    base_branch: null,
    task_key: null,
    task_project_id: null,
  };
}

let tasksByPath: Record<string, TaskItem[]>;

function baseConfig(overrides: Record<string, unknown> = {}) {
  return {
    appearance: { mode: "light", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: {},
    default_provider: "kiro",
    ...overrides,
  };
}

const noop = () => {};

async function mountSidebar(config: Record<string, unknown>, props: Record<string, unknown> = {}) {
  configGet.mockResolvedValue(baseConfig(config));
  await loadSettings();
  await loadProjects();
  await loadSessions();
  await loadTasks([alpha.path, beta.path]);
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(UnifiedSidebar, {
    target,
    props: {
      renamingSessionId: null,
      onAddProject: noop,
      onSelectSession: noop,
      onArchiveSession: noop,
      onDeleteSession: noop,
      onRestartSession: noop,
      onOpenPreferences: noop,
      onRenameSession: noop,
      onStartRename: noop,
      onDeleteProject: noop,
      onEditProject: noop,
      onPickTask: noop,
      ...props,
    },
  });
  flushSync();
  return { target, component };
}

function texts(root: HTMLElement, selector: string): string[] {
  return [...root.querySelectorAll<HTMLElement>(selector)].map((el) =>
    el.textContent!.replace(/\s+/g, " ").trim(),
  );
}

function navButtons(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>("nav [data-nav-index]")];
}

describe("UnifiedSidebar grouped by status", () => {
  let mounted: { target: HTMLElement; component: ReturnType<typeof mount> } | undefined;

  beforeEach(() => {
    localStorage.clear();
    _resetForTests();
    HTMLElement.prototype.scrollIntoView = vi.fn();
    tasksByPath = {
      [alpha.path]: [task("A-1", "in_progress", 1), task("A-2", "todo"), task("A-3", "done")],
      [beta.path]: [task("B-1", "in_progress", 2), task("B-2", "in_review")],
    };
    projectList.mockResolvedValue([alpha, beta]);
    sessionList.mockResolvedValue([session("scratch", "beta")]);
    taskListAll.mockImplementation(async (path: string) => tasksByPath[path] ?? []);
    taskMove.mockImplementation(async (key: string, status: string, path: string) => {
      tasksByPath[path] = tasksByPath[path].map((t) => (t.key === key ? { ...t, status } : t));
    });
    configUpdate.mockResolvedValue(undefined);
  });

  afterEach(async () => {
    if (mounted) {
      unmount(mounted.component);
      mounted.target.remove();
    }
    mounted = undefined;
    _resetForTests();
    projectList.mockResolvedValue([]);
    await loadProjects();
    await loadTasks([]);
    focusTerminal();
    setSelectedIndex(0);
    vi.clearAllMocks();
  });

  it("shows sessions first, then flat status groups across projects with project labels, Done collapsed", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const nav = mounted.target.querySelector("nav")!;

    expect(navButtons(nav).map((b) => b.textContent!.replace(/\s+/g, " ").trim())).toEqual([
      "Sessions 1",
      "beta-service scratch",
      "Running 2",
      "beta-service B-1 Title B-1",
      "alpha A-1 Title A-1",
      "Needs review 1",
      "beta-service B-2 Title B-2",
      "To do 1",
      "alpha A-2 Title A-2",
      "Done 1",
    ]);
  });

  it("hides task keys and project labels when those options are off", async () => {
    mounted = await mountSidebar({
      sidebar_group_by: "status",
      hide_task_keys: true,
      hide_project_labels: true,
    });
    const nav = mounted.target.querySelector("nav")!;

    expect(texts(nav, "[data-nav-index]").slice(2, 4)).toEqual(["Running 2", "Title B-1"]);
  });

  it("opens view options from right-click on empty space and switches back to project grouping", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const nav = mounted.target.querySelector("nav")!;

    nav.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }),
    );
    flushSync();
    const groupByProject = [
      ...document.querySelectorAll<HTMLButtonElement>("[role='menuitemradio']"),
    ].find((b) => b.textContent!.trim() === "Group by project")!;
    expect(groupByProject.getAttribute("aria-checked")).toBe("false");

    groupByProject.click();
    flushSync();

    expect(getSettings().sidebar_group_by).toBe("project");
    expect(configUpdate).toHaveBeenCalledWith(
      expect.objectContaining({ sidebar_group_by: "project" }),
    );
    expect(texts(nav, "[data-nav-index]")[0]).toBe("alpha 3");
  });

  it("keeps row context menus instead of view options", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });

    const row = navButtons(mounted.target).find((b) => b.textContent!.includes("B-1"))!;
    row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();

    expect(document.querySelector("[role='menuitemradio'], [role='menuitemcheckbox']")).toBeNull();
    expect(texts(document.body, "[role='menuitem']")).toContain("Edit task");
  });

  it("hands task edits from the keyboard and context menu to the host dialog", async () => {
    const onEditTask = vi.fn();
    mounted = await mountSidebar({ sidebar_group_by: "status" }, { onEditTask });
    focusSidebar();
    flushSync();
    setSelectedIndex(navButtons(mounted.target).findIndex((b) => b.textContent!.includes("A-2")));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", bubbles: true, cancelable: true }));

    const row = navButtons(mounted.target).find((b) => b.textContent!.includes("B-1"))!;
    row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    [...document.querySelectorAll<HTMLElement>("[role='menuitem']")].find((el) => el.textContent === "Edit task")!.click();
    flushSync();

    expect(onEditTask.mock.calls.map(([t, p]) => [t.key, p.path])).toEqual([
      ["A-2", alpha.path],
      ["B-1", beta.path],
    ]);
    expect(document.querySelector("[aria-label='Edit Task']")).toBeNull();
  });

  it("keeps the keyboard selection on a task after its status changes", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    focusSidebar();
    // Let the auto-focus on the active session settle before choosing a row.
    flushSync();
    const index = navButtons(mounted.target).findIndex((b) => b.textContent!.includes("A-2"));
    setSelectedIndex(index);

    for (const key of ["s", "p"]) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    }

    await vi.waitFor(() => {
      const selected = navButtons(mounted!.target)[getSelectedIndex()];
      expect(selected.textContent).toContain("A-2");
      expect(texts(mounted!.target, "nav [data-nav-index]").slice(2, 6)).toEqual([
        "Running 3",
        "beta-service B-1 Title B-1",
        "alpha A-1 Title A-1",
        "alpha A-2 Title A-2",
      ]);
    });
    expect(taskMove).toHaveBeenCalledWith("A-2", "in_progress", alpha.path);
  });

  it("reports a failed status change and keeps the selection", async () => {
    taskMove.mockRejectedValueOnce(new Error("transition refused"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    focusSidebar();
    // Let the auto-focus on the active session settle before choosing a row.
    flushSync();
    const index = navButtons(mounted.target).findIndex((b) => b.textContent!.includes("A-2"));
    setSelectedIndex(index);

    for (const key of ["s", "p"]) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    }

    await vi.waitFor(() => expect(showSnackbar).toHaveBeenCalledWith("Failed to move task."));
    expect(getSelectedIndex()).toBe(index);
  });

  it("drives the view menu from the keyboard without moving the sidebar selection", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    focusSidebar();
    // Let the auto-focus on the active session settle before choosing a row.
    flushSync();
    setSelectedIndex(2);
    mounted.target
      .querySelector("nav")!
      .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    await vi.waitFor(() =>
      expect(document.activeElement?.textContent?.trim()).toBe("Group by project"),
    );

    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }),
    );
    expect(document.activeElement?.textContent?.trim()).toBe("Group by status");
    expect(getSelectedIndex()).toBe(2);

    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    flushSync();
    expect(document.querySelector("[role='menu']")).toBeNull();
  });

  it("ignores right-clicks inside rows that have no menu of their own", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const li = mounted.target.querySelector("nav li")!;
    li.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    expect(document.querySelector("[role='menuitemradio']")).toBeNull();
  });

  it("stores only collapse states that differ from the default", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const doneHeader = () =>
      navButtons(mounted!.target).find((b) => b.textContent!.trim().startsWith("Done"))!;

    doneHeader().click();
    flushSync();
    expect(JSON.parse(localStorage.getItem("planeai:layout:sidebar-collapsed")!)).toEqual({
      "status:done": false,
    });

    doneHeader().click();
    flushSync();
    expect(JSON.parse(localStorage.getItem("planeai:layout:sidebar-collapsed")!)).toEqual({});
  });

  it("remembers collapsed groups across remounts", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    navButtons(mounted.target)
      .find((b) => b.textContent!.trim().startsWith("Running"))!
      .click();
    flushSync();
    expect(JSON.parse(localStorage.getItem("planeai:layout:sidebar-collapsed")!)).toEqual({
      "status:in_progress": true,
    });

    unmount(mounted.component);
    mounted.target.remove();
    mounted = await mountSidebar({ sidebar_group_by: "status" });

    expect(texts(mounted.target, "nav [data-nav-index]").slice(2, 4)).toEqual([
      "Running 2",
      "Needs review 1",
    ]);
  });
});
