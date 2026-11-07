import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import type { Project, Session, StoredSnooze, TaskItem, TaskSnooze } from "../../lib/types";

const { configGet, projectList, sessionList, showToast, taskListAll, taskMove, taskWake } =
  vi.hoisted(() => ({
    configGet: vi.fn(),
    projectList: vi.fn(),
    sessionList: vi.fn(),
    showToast: vi.fn(),
    taskListAll: vi.fn(),
    taskMove: vi.fn(),
    taskWake: vi.fn(),
  }));

vi.mock("../../lib/api", () => ({
  config: { get: configGet, update: vi.fn(), refresh: configGet },
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
  tasks: { listAll: taskListAll, move: taskMove, wake: taskWake },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../lib/toasts.svelte", () => ({ showToast, dismissToast: vi.fn() }));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ onCloseRequested: vi.fn().mockResolvedValue(() => {}) })),
}));

import UnifiedSidebar from "../UnifiedSidebar.svelte";
import { focusTerminal } from "../../lib/focus.svelte";
import { loadProjects } from "../../lib/project-store.svelte";
import { _resetForTests, loadSessions } from "../../lib/session-orchestrator.svelte";
import { setSelectedIndex } from "../../lib/sidebar-nav.svelte";
import { _resetForTests as resetSidebarView } from "../../lib/sidebar-model-store.svelte";
import { getSnoozeMenu, closeSnoozeMenu } from "../../lib/snooze-picker.svelte";
import { syncSnoozeClock } from "../../lib/snooze-clock.svelte";
import { loadTasks } from "../../lib/task-store.svelte";
import { commitPendingDone } from "../../lib/task-moves";
import { loadSettings } from "../../lib/settings.svelte";
import { taskDrag } from "../../lib/task-drag.svelte";

const alpha: Project = {
  id: "alpha",
  name: "Alpha",
  path: "/repos/alpha",
  hidden: false,
  path_missing: false,
};
const beta: Project = {
  id: "beta",
  name: "Beta",
  path: "/repos/beta",
  hidden: false,
  path_missing: false,
};

function task(key: string, status: string, snooze: Partial<TaskSnooze> = {}): TaskItem {
  return {
    key,
    title: `Title ${key}`,
    status,
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
    last_visited_at: null,
    last_activity_at: null,
    snoozed_until: null,
    snoozed_at: null,
    woke_at: null,
    snooze_rev: 0,
    created_at: "2026-11-01T09:00:00.000Z",
    ...snooze,
  };
}

const AWAKE: StoredSnooze = { snoozed_until: null, snoozed_at: null, woke_at: null, snooze_rev: 1 };
const snoozed = {
  snoozed_until: new Date(Date.now() + 3_600_000).toISOString(),
  snoozed_at: new Date(Date.now() - 60_000).toISOString(),
};

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
const noop = () => {};

async function mountSidebar(config: Record<string, unknown>, props: Record<string, unknown> = {}) {
  configGet.mockResolvedValue({
    appearance: { mode: "light", theme: "default" },
    terminal: { font_family: "Menlo", font_size: 14, option_as_meta: true },
    providers: {},
    default_provider: "kiro",
    ...config,
  });
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
      onEditTask: noop,
      onPickTask: noop,
      ...props,
    },
  });
  flushSync();
  return { target, component };
}

function navButtons(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>("nav [data-nav-index]")];
}

/** A header in brackets with its text, a row by its title. */
function labelOf(button: HTMLElement): string {
  if (button.hasAttribute("aria-expanded")) {
    return `[${button.textContent!.replace(/\s+/g, " ").trim()}]`;
  }
  const labelledBy = button.getAttribute("aria-labelledby");
  const content = labelledBy ? document.getElementById(labelledBy)! : button;
  const title = content.querySelector("span.text-\\[13px\\], span.text-\\[12\\.5px\\]");
  return (title ?? content).textContent!.trim();
}

const navLabels = (root: HTMLElement) => navButtons(root).map(labelOf);

function rowButton(root: HTMLElement, title: string): HTMLElement {
  const button = navButtons(root).find((b) => labelOf(b) === title);
  if (!button) throw new Error(`no row titled ${title}`);
  return button;
}

function laneOf(root: HTMLElement, lane: string, projectId?: string): HTMLElement {
  const project = projectId ? `[data-task-lane-project="${projectId}"]` : "";
  const element = root.querySelector<HTMLElement>(`[data-task-lane="${lane}"]${project}`);
  if (!element) throw new Error(`no ${lane} lane`);
  return element;
}

/**
 * Presses a row and drags it 40px down; what is under the pointer is whatever `over` returns. The
 * first move starts the drag, which shows every lane; the second finds the one under the pointer.
 */
function dragRow(root: HTMLElement, title: string, over: () => Element | null): void {
  const row = rowButton(root, title);
  row.dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: 50, clientY: 100 }),
  );
  document.elementFromPoint = () => null;
  window.dispatchEvent(new MouseEvent("pointermove", { clientX: 50, clientY: 120 }));
  flushSync();
  document.elementFromPoint = over;
  window.dispatchEvent(new MouseEvent("pointermove", { clientX: 50, clientY: 140 }));
  flushSync();
}

function release(): void {
  window.dispatchEvent(new MouseEvent("pointerup", { clientX: 50, clientY: 140 }));
  flushSync();
}

const badges = (root: HTMLElement) =>
  [...root.querySelectorAll<HTMLElement>("[data-lane-drop]")].map(
    (badge) => `${badge.dataset.laneDrop}: ${badge.textContent!.trim()}`,
  );

describe("dragging a task row to a lane", () => {
  let mounted: { target: HTMLElement; component: ReturnType<typeof mount> } | undefined;

  beforeEach(() => {
    localStorage.clear();
    resetSidebarView();
    _resetForTests();
    HTMLElement.prototype.scrollIntoView = vi.fn();
    tasksByPath = {
      [alpha.path]: [task("A-1", "in_progress"), task("A-2", "todo"), task("A-3", "done")],
      [beta.path]: [task("B-1", "in_review")],
    };
    projectList.mockResolvedValue([alpha, beta]);
    sessionList.mockResolvedValue([session("scratch", "beta")]);
    taskListAll.mockImplementation(async (path: string) => tasksByPath[path] ?? []);
    taskMove.mockImplementation(async (key: string, status: string, path: string) => {
      tasksByPath[path] = tasksByPath[path].map((t) => (t.key === key ? { ...t, status } : t));
    });
    taskWake.mockImplementation(async (_projectId: string, key: string) => {
      for (const path of Object.keys(tasksByPath)) {
        tasksByPath[path] = tasksByPath[path].map((t) => (t.key === key ? { ...t, ...AWAKE } : t));
      }
      return AWAKE;
    });
    syncSnoozeClock();
  });

  afterEach(async () => {
    release();
    closeSnoozeMenu();
    await commitPendingDone();
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

  it("moves a Cards task to the shelf it is dropped on, naming the outcome while it hovers", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "Title A-2", () => laneOf(root, "in_progress"));
    expect(badges(root)).toEqual(["accepts: Running"]);
    release();

    await vi.waitFor(() =>
      expect(taskMove).toHaveBeenCalledWith("A-2", "in_progress", "/repos/alpha"),
    );
    expect(showToast).not.toHaveBeenCalled();
  });

  it("shows every shelf while a task is dragged, so an empty lane can take it", async () => {
    tasksByPath[alpha.path] = [task("A-2", "todo")];
    tasksByPath[beta.path] = [];
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    expect(navLabels(root)).toEqual(["scratch", "[To do 1]", "Title A-2"]);

    dragRow(root, "Title A-2", () => null);
    expect(navLabels(root)).toEqual([
      "scratch",
      "[Running 0]",
      "[To do 1]",
      "Title A-2",
      "[Snoozed 0]",
      "[Done 0]",
    ]);
    release();
    expect(navLabels(root)).toEqual(["scratch", "[To do 1]", "Title A-2"]);
  });

  it("drops a task on the top area to send it for review", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "Title A-1", () => laneOf(root, "in_review"));
    expect(badges(root)).toEqual(["accepts: Needs review"]);
    release();

    await vi.waitFor(() =>
      expect(taskMove).toHaveBeenCalledWith("A-1", "in_review", "/repos/alpha"),
    );
  });

  it("leaves a task dropped back on its own lane alone", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "Title A-2", () => laneOf(root, "todo"));
    expect(badges(root)).toEqual([]);
    release();
    await Promise.resolve();

    expect(taskMove).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it("refuses another project's folder, with a badge and a toast", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "project" });
    const root = mounted.target;

    dragRow(root, "Title A-1", () => laneOf(root, "in_review", "beta"));
    expect(badges(root)).toEqual(["refuses: A-1 belongs to Alpha."]);
    release();

    expect(taskMove).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith({
      kind: "info",
      message: "A-1 belongs to Alpha.",
      autoDismissMs: 3000,
    });
  });

  it("refuses to snooze a done task", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    root.querySelector<HTMLButtonElement>('[data-task-lane="done"] button')!.click();
    flushSync();

    dragRow(root, "Title A-3", () => laneOf(root, "snoozed"));
    expect(badges(root)).toEqual(["refuses: Done tasks cannot be snoozed."]);
    release();

    expect(getSnoozeMenu()).toBeNull();
    expect(showToast).toHaveBeenCalledWith({
      kind: "info",
      message: "Done tasks cannot be snoozed.",
      autoDismissMs: 3000,
    });
  });

  it("opens the snooze picker for a task dropped on Snoozed", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "Title A-2", () => laneOf(root, "snoozed"));
    expect(badges(root)).toEqual(["accepts: Snooze…"]);
    release();

    expect(getSnoozeMenu()?.target.task.key).toBe("A-2");
    expect(taskMove).not.toHaveBeenCalled();
  });

  it("wakes a snoozed task dropped on its own status lane", async () => {
    tasksByPath[alpha.path] = [task("A-1", "in_progress", snoozed), task("A-2", "todo")];
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    root.querySelector<HTMLButtonElement>('[data-task-lane="snoozed"] button')!.click();
    flushSync();

    dragRow(root, "Title A-1", () => laneOf(root, "in_progress"));
    expect(badges(root)).toEqual(["accepts: Wake"]);
    release();

    await vi.waitFor(() => expect(taskWake).toHaveBeenCalledWith("alpha", "A-1"));
    expect(taskMove).not.toHaveBeenCalled();
  });

  it("wakes and moves a snoozed task dropped on another status lane", async () => {
    tasksByPath[alpha.path] = [task("A-1", "in_progress", snoozed), task("A-2", "todo")];
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    root.querySelector<HTMLButtonElement>('[data-task-lane="snoozed"] button')!.click();
    flushSync();

    dragRow(root, "Title A-1", () => laneOf(root, "todo"));
    expect(badges(root)).toEqual(["accepts: Wake to To do"]);
    release();

    await vi.waitFor(() => expect(taskMove).toHaveBeenCalledWith("A-1", "todo", "/repos/alpha"));
    expect(taskWake).toHaveBeenCalledWith("alpha", "A-1");
  });

  it("moves a Classic task between its project's groups, offering the empty ones too", async () => {
    tasksByPath[alpha.path] = [task("A-1", "in_progress"), task("A-2", "todo")];
    tasksByPath[beta.path] = [];
    mounted = await mountSidebar({ sidebar_layout: "classic", sidebar_group_by: "project" });
    const root = mounted.target;

    dragRow(root, "Title A-2", () => laneOf(root, "in_review", "alpha"));
    expect(navLabels(root)).toEqual([
      "[Alpha 2]",
      "[Running 1]",
      "Title A-1",
      "[Needs review 0 Needs review]",
      "[To do 1]",
      "Title A-2",
      "[Done 0]",
      "[Snoozed 0]",
      "[Beta 1]",
      "scratch",
    ]);
    expect(badges(root)).toEqual(["accepts: Needs review"]);
    release();

    await vi.waitFor(() =>
      expect(taskMove).toHaveBeenCalledWith("A-2", "in_review", "/repos/alpha"),
    );
  });

  it("offers every status section of the Classic status layout while dragging", async () => {
    tasksByPath[alpha.path] = [task("A-2", "todo")];
    tasksByPath[beta.path] = [];
    mounted = await mountSidebar({ sidebar_layout: "classic", sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "Title A-2", () => laneOf(root, "done"));
    expect(navLabels(root)).toEqual([
      "[Sessions 1]",
      "Beta scratch",
      "[Running 0]",
      "[Needs review 0]",
      "[To do 1]",
      "Title A-2",
      "[Snoozed 0]",
      "[Done Done 0]",
    ]);
    release();

    expect(taskDrag.current).toBeNull();
    // Done shows at once and starts collapsed; the backend move waits out the undo window.
    expect(navLabels(root)).toEqual(["[Sessions 1]", "Beta scratch", "[Done 1]"]);
    expect(taskMove).not.toHaveBeenCalled();
    await commitPendingDone();
    expect(taskMove).toHaveBeenCalledWith("A-2", "done", "/repos/alpha");
  });

  it("hands a task dropped on a pane to the app", async () => {
    const reference = vi.fn();
    mounted = await mountSidebar({ sidebar_group_by: "status" }, { onReferenceTask: reference });
    const root = mounted.target;
    const pane = document.createElement("div");
    pane.dataset.paneDrop = "pane-1";
    document.body.append(pane);

    dragRow(root, "Title A-2", () => pane);
    release();
    pane.remove();

    expect(reference).toHaveBeenCalledWith(
      {
        projectId: "alpha",
        projectName: "Alpha",
        repoPath: "/repos/alpha",
        key: "A-2",
        title: "Title A-2",
        status: "todo",
        lane: "todo",
      },
      "pane-1",
    );
    expect(taskMove).not.toHaveBeenCalled();
  });

  it("lifts the dragged row above its siblings and puts it back on release", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    const lifted = () =>
      [...root.querySelectorAll<HTMLElement>(".task-drag-lifted")].map((row) =>
        row.querySelector("[data-nav-index]")!.textContent!.trim(),
      );

    dragRow(root, "Title A-2", () => null);
    expect(lifted()).toEqual(["Alpha A-2 Title A-2"]);
    release();

    expect(lifted()).toEqual([]);
  });

  /** A list 600px tall at the top of the viewport, scrollable, with the row's slot 300px down. */
  function layOut(root: HTMLElement, title: string) {
    const rect = (top: number, height: number) =>
      ({ left: 0, top, width: 200, height, right: 200, bottom: top + height }) as DOMRect;
    const list = root.querySelector<HTMLElement>("nav")!;
    list.getBoundingClientRect = () => rect(0, 600);
    let scrollTop = 0;
    Object.defineProperty(list, "scrollTop", {
      get: () => scrollTop,
      set: (value: number) => {
        scrollTop = value;
      },
    });
    const row = () => rowButton(root, title).closest<HTMLElement>(".group")!;
    // The rect includes the row's transform, as a browser's does.
    row().getBoundingClientRect = function (this: HTMLElement) {
      const lifted = /translateY\((-?\d+)px\)/.exec(this.style.transform);
      return rect(300 + (lifted ? Number(lifted[1]) : 0) - scrollTop, 32);
    };
    const moveTo = (x: number, y: number) => {
      window.dispatchEvent(new MouseEvent("pointermove", { clientX: x, clientY: y }));
      flushSync();
    };
    const scrollTo = (top: number) => {
      list.scrollTop = top;
      list.dispatchEvent(new Event("scroll"));
      flushSync();
    };
    return { row, moveTo, scrollTo };
  }

  it("follows the pointer inside the list, stops at its edges, and glides home outside it", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    const { row, moveTo } = layOut(root, "Title A-2");

    // The press is at y 100, the drag's second move at y 140, where the row is measured.
    dragRow(root, "Title A-2", () => null);
    await new Promise((resolve) => requestAnimationFrame(resolve));

    moveTo(50, 180);
    expect([row().style.transform, row().style.transition]).toEqual(["translateY(40px)", "none"]);
    moveTo(50, 900);
    expect(row().style.transform).toBe("translateY(268px)");
    moveTo(500, 180);
    expect([row().style.transform, row().style.transition]).toEqual([
      "translateY(0)",
      "transform 150ms ease-out",
    ]);
    release();
    expect([row().style.transform, row().className.includes("task-drag-lifted")]).toEqual([
      "",
      false,
    ]);
  });

  it("keeps the lifted row under the pointer across a store refresh and a scroll of the list", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    const { row, moveTo, scrollTo } = layOut(root, "Title A-2");

    dragRow(root, "Title A-2", () => null);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    moveTo(50, 180);
    expect(row().style.transform).toBe("translateY(40px)");

    // The rows are rebuilt from the store; the lifted one is measured no second time.
    tasksByPath[beta.path] = [task("B-1", "in_review"), task("B-2", "todo")];
    await loadTasks([alpha.path, beta.path]);
    flushSync();
    await new Promise((resolve) => requestAnimationFrame(resolve));
    expect([row().style.transform, row().className.includes("task-drag-lifted")]).toEqual([
      "translateY(40px)",
      true,
    ]);
    moveTo(50, 190);
    expect(row().style.transform).toBe("translateY(50px)");

    // The list scrolls 20px under the still pointer: the slot moves up, the row stays put.
    scrollTo(20);
    expect(row().style.transform).toBe("translateY(70px)");
    moveTo(50, 200);
    expect(row().style.transform).toBe("translateY(80px)");
  });

  it("decides a drop from the task as the store has it, not the snapshot the press took", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "Title A-2", () => laneOf(root, "in_progress"));
    tasksByPath[alpha.path] = tasksByPath[alpha.path].map((t) =>
      t.key === "A-2" ? { ...t, status: "in_progress" } : t,
    );
    await loadTasks([alpha.path, beta.path]);
    release();
    await Promise.resolve();

    expect(taskMove).not.toHaveBeenCalled();
  });

  it("does not drag session rows", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;

    dragRow(root, "scratch", () => laneOf(root, "done"));
    expect(taskDrag.current).toBeNull();
    expect(document.body.classList.contains("task-dragging")).toBe(false);
  });
});
