import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import type {
  PluginInventory,
  PluginUiContribution,
  Project,
  Session,
  TaskItem,
} from "../../lib/types";

const {
  configGet,
  configUpdate,
  projectList,
  sessionList,
  taskListAll,
  taskMove,
  taskMarkVisited,
  taskMarkUnread,
  taskWake,
} = vi.hoisted(() => ({
  configGet: vi.fn(),
  configUpdate: vi.fn(),
  projectList: vi.fn(),
  sessionList: vi.fn(),
  taskListAll: vi.fn(),
  taskMove: vi.fn(),
  taskMarkVisited: vi.fn(),
  taskMarkUnread: vi.fn(),
  taskWake: vi.fn(),
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
  tasks: {
    listAll: taskListAll,
    move: taskMove,
    markVisited: taskMarkVisited,
    markUnread: taskMarkUnread,
    wake: taskWake,
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../lib/toasts.svelte", () => ({ showToast: vi.fn(), dismissToast: vi.fn() }));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ onCloseRequested: vi.fn().mockResolvedValue(() => {}) })),
}));

import UnifiedSidebar from "../UnifiedSidebar.svelte";
import { focusSidebar, focusTerminal } from "../../lib/focus.svelte";
import { loadProjects } from "../../lib/project-store.svelte";
import { _resetForTests, loadSessions } from "../../lib/session-orchestrator.svelte";
import { getSelectedIndex, setSelectedIndex } from "../../lib/sidebar-nav.svelte";
import {
  _resetForTests as resetSidebarView,
  getVisibleNavigationOrder,
} from "../../lib/sidebar-model-store.svelte";
import { loadTasks } from "../../lib/task-store.svelte";
import { commitPendingDone } from "../../lib/task-moves";
import { loadSettings } from "../../lib/settings.svelte";
import { setLayoutWidth } from "../../lib/layout-state";
import { syncSnoozeClock } from "../../lib/snooze-clock.svelte";

const alpha: Project = {
  id: "alpha",
  name: "alpha",
  path: "/repos/alpha",
  hidden: false,
  path_missing: false,
};
const beta: Project = {
  id: "beta",
  name: "beta-service",
  path: "/repos/beta",
  hidden: false,
  path_missing: false,
};

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
    last_visited_at: null,
    last_activity_at: null,
    snoozed_until: null,
    snoozed_at: null,
    woke_at: null,
    snooze_rev: 0,
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
  return (content.querySelector("span.text-\\[13px\\]") ?? content).textContent!.trim();
}

function navLabels(root: HTMLElement): string[] {
  return navButtons(root).map(labelOf);
}

function rowOf(root: HTMLElement, title: string): HTMLElement {
  const button = navButtons(root).find((b) => labelOf(b) === title);
  if (!button) throw new Error(`no row titled ${title}`);
  return button.parentElement!;
}

function clickAction(root: HTMLElement, title: string, action: string): void {
  rowOf(root, title).querySelector<HTMLButtonElement>(`button[aria-label="${action}"]`)!.click();
  flushSync();
}

function press(key: string): void {
  window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  flushSync();
}

describe("UnifiedSidebar in the Cards layout", () => {
  let mounted: { target: HTMLElement; component: ReturnType<typeof mount> } | undefined;

  // jsdom lacks the Web Animations API that the row motion runs on; every animation finishes at once.
  beforeAll(() => {
    Element.prototype.getAnimations = () => [];
    Element.prototype.animate = function () {
      const animation = {
        currentTime: 0,
        playState: "finished",
        onfinish: null as (() => void) | null,
      };
      queueMicrotask(() => animation.onfinish?.());
      return Object.assign(animation, { cancel() {} }) as unknown as Animation;
    };
  });

  afterAll(() => {
    delete (Element.prototype as Partial<Element>).animate;
    delete (Element.prototype as Partial<Element>).getAnimations;
  });

  beforeEach(() => {
    localStorage.clear();
    resetSidebarView();
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

  it("shows Unread on a task its agent finished since it was last opened, with or without a session", async () => {
    const finished = { last_activity_at: "2026-11-03T10:00:00Z", last_visited_at: null };
    tasksByPath[alpha.path] = [
      { ...task("A-1", "in_progress", 1), ...finished },
      { ...task("A-2", "todo"), ...finished },
    ];
    tasksByPath[beta.path] = [
      { ...task("B-1", "in_progress", 2), ...finished, last_visited_at: "2026-11-03T10:05:00Z" },
    ];
    mounted = await mountSidebar({ sidebar_group_by: "status" });

    const statusOf = (title: string) => {
      const row = rowOf(mounted!.target, title);
      return {
        unread: row.textContent!.includes("Unread"),
        tooltip: row.querySelector("[title^='Unread']")?.getAttribute("title") ?? null,
      };
    };
    expect([statusOf("Title A-1"), statusOf("Title A-2"), statusOf("Title B-1")]).toEqual([
      { unread: true, tooltip: "Unread: the agent finished since you last opened this task" },
      { unread: true, tooltip: "Unread: the agent finished since you last opened this task" },
      { unread: false, tooltip: null },
    ]);
  });

  it("marks a task unread or read from its context menu and with u", async () => {
    const activity = "2026-11-03T10:00:00.000Z";
    tasksByPath[alpha.path] = [
      { ...task("A-1", "in_progress", 1), last_activity_at: activity, last_visited_at: null },
      {
        ...task("A-2", "in_progress"),
        last_activity_at: activity,
        last_visited_at: "2026-11-03T10:05:00.000Z",
      },
      task("A-3", "in_progress"),
    ];
    taskMarkVisited.mockResolvedValue({
      last_visited_at: "2026-11-03T10:06:00.000Z",
      last_activity_at: activity,
    });
    taskMarkUnread.mockResolvedValue({
      last_visited_at: "2026-11-03T09:59:59.999Z",
      last_activity_at: activity,
    });
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    const menuOf = (title: string) => {
      rowOf(root, title).dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
      flushSync();
      const labels = [...document.querySelectorAll<HTMLElement>("[role='menuitem']")].map((el) =>
        el.textContent!.trim(),
      );
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      flushSync();
      return labels.filter((label) => label.startsWith("Mark as"));
    };
    const unread = (title: string) => rowOf(root, title).textContent!.includes("Unread");

    expect([menuOf("Title A-1"), menuOf("Title A-2"), menuOf("Title A-3")]).toEqual([
      ["Mark as read"],
      ["Mark as unread"],
      [],
    ]);

    rowOf(root, "Title A-2").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    flushSync();
    [...document.querySelectorAll<HTMLElement>("[role='menuitem']")]
      .find((el) => el.textContent!.trim() === "Mark as unread")!
      .click();
    focusSidebar();
    setSelectedIndex(navButtons(root).findIndex((b) => labelOf(b) === "Title A-1"));
    press("u");
    await vi.waitFor(() =>
      expect([unread("Title A-1"), unread("Title A-2")]).toEqual([false, true]),
    );

    expect([taskMarkUnread.mock.calls, taskMarkVisited.mock.calls]).toEqual([
      [["alpha", "A-2"]],
      [["alpha", "A-1"]],
    ]);
  });

  it("sets snoozed tasks aside in a collapsed shelf, counting down, and keeps the open one listed", async () => {
    const snoozedUntil = (ms: number) => ({
      snoozed_until: new Date(Date.now() + ms).toISOString(),
      snoozed_at: new Date(Date.now() - 60_000).toISOString(),
    });
    tasksByPath[alpha.path] = [
      { ...task("A-1", "in_progress", 1), ...snoozedUntil(3 * 3_600_000 + 30_000) },
      { ...task("A-2", "todo"), ...snoozedUntil(45 * 60_000 + 30_000) },
      task("A-3", "done"),
    ];
    tasksByPath[beta.path] = [{ ...task("B-2", "in_review"), ...snoozedUntil(3_600_000) }];
    // App starts the clock at launch, which reads the time.
    syncSnoozeClock();
    mounted = await mountSidebar(
      { sidebar_group_by: "status" },
      { selectedTaskWorkspace: { projectId: "alpha", taskKey: "A-1" } },
    );
    const root = mounted.target;

    expect(navLabels(root)).toEqual(["scratch", "[Snoozed 3]", "Title A-1", "[Done 1]"]);
    expect(rowOf(root, "Title A-1").textContent).toContain("3h");

    root.querySelector<HTMLButtonElement>("button[aria-expanded='false']")!.click();
    flushSync();

    expect(navLabels(root).slice(1, 5)).toEqual([
      "[Snoozed 3]",
      "Title A-2",
      "Title B-2",
      "Title A-1",
    ]);
    const countdown = rowOf(root, "Title A-2").querySelector("[title^='Wakes ']")!;
    expect(countdown.textContent).toBe("45m");
  });

  it("wakes a snoozed task from its hover action, back to its status", async () => {
    const snooze = {
      snoozed_until: new Date(Date.now() + 3_600_000).toISOString(),
      snoozed_at: new Date(Date.now() - 60_000).toISOString(),
    };
    tasksByPath[alpha.path] = [{ ...task("A-2", "todo"), ...snooze }];
    tasksByPath[beta.path] = [];
    taskWake.mockResolvedValue({
      snoozed_until: null,
      snoozed_at: null,
      woke_at: null,
      snooze_rev: 1,
    });
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    root.querySelector<HTMLButtonElement>("button[aria-expanded='false']")!.click();
    flushSync();

    clickAction(root, "Title A-2", "Wake now");
    await vi.waitFor(() => expect(navLabels(root)).toEqual(["scratch", "[To do 1]", "Title A-2"]));
    expect(taskWake.mock.calls).toEqual([["alpha", "A-2"]]);
  });

  it("offers Snooze or Wake in the task menu, and z wakes a snoozed row", async () => {
    tasksByPath[alpha.path] = [
      task("A-1", "in_progress", 1),
      {
        ...task("A-2", "todo"),
        snoozed_until: new Date(Date.now() + 3_600_000).toISOString(),
        snoozed_at: new Date(Date.now() - 60_000).toISOString(),
      },
    ];
    tasksByPath[beta.path] = [];
    taskWake.mockResolvedValue({
      snoozed_until: null,
      snoozed_at: null,
      woke_at: null,
      snooze_rev: 1,
    });
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const root = mounted.target;
    root.querySelector<HTMLButtonElement>("button[aria-expanded='false']")!.click();
    flushSync();
    const menuOf = (title: string) => {
      navButtons(root)
        .find((button) => labelOf(button) === title)!
        .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
      flushSync();
      const labels = [...document.querySelectorAll<HTMLElement>("[role='menuitem']")].map((el) =>
        el.textContent!.replace("›", "").trim(),
      );
      document
        .querySelector("[role='menu']")!
        .dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      flushSync();
      return labels.filter((label) => label === "Snooze" || label === "Wake");
    };

    expect([menuOf("Title A-1"), menuOf("Title A-2")]).toEqual([["Snooze"], ["Wake"]]);

    focusSidebar();
    setSelectedIndex(navLabels(root).indexOf("Title A-2"));
    press("z");

    await vi.waitFor(() => expect(taskWake.mock.calls).toEqual([["alpha", "A-2"]]));
  });

  it("is the default, with sessions and review tasks on top and shelves in status order", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });

    expect(navLabels(mounted.target)).toEqual([
      "scratch",
      "Title B-2",
      "[Running 2]",
      "Title B-1",
      "Title A-1",
      "[To do 1]",
      "Title A-2",
      "[Done 1]",
    ]);
  });

  it("drops the project line from cards when project labels are hidden", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status", hide_project_labels: true });

    expect(rowOf(mounted.target, "scratch").textContent).not.toContain("beta-service");
    expect(rowOf(mounted.target, "Title B-1").textContent).not.toContain("beta-service");
  });

  it("shows the project line on shelf cards when grouped by project unless labels are hidden", async () => {
    mounted = await mountSidebar({});
    expect(rowOf(mounted.target, "Title B-1").textContent).toContain("beta-service");
    unmount(mounted.component);
    mounted.target.remove();

    mounted = await mountSidebar({ hide_project_labels: true });
    expect(rowOf(mounted.target, "Title B-1").textContent).not.toContain("beta-service");
  });

  it("shows New project as an unpadded icon button below 400px and labelled above", async () => {
    const newProject = () =>
      mounted!.target.querySelector<HTMLButtonElement>("button[aria-label='New project']")!;

    mounted = await mountSidebar({});
    expect(newProject().textContent!.trim()).toBe("");
    expect(newProject().title).toMatch(/^New project \(.+⇧N\)$/);
    expect(newProject().classList).toContain("px-0");
    expect(newProject().classList).not.toContain("px-3");
    unmount(mounted.component);
    mounted.target.remove();

    setLayoutWidth("sidebar", 400);
    mounted = await mountSidebar({});
    expect(newProject().textContent!.replace(/\s+/g, " ").trim()).toMatch(/^New project .+⇧N$/);
    expect(newProject().title).toBe("");
    expect(newProject().classList).toContain("px-3");
    expect(newProject().classList).not.toContain("px-0");
  });

  it.each(["cards", "classic"])(
    "renames a session from its input in the %s layout",
    async (layout) => {
      const onRenameSession = vi.fn();
      const onStartRename = vi.fn();
      mounted = await mountSidebar(
        { sidebar_layout: layout, sidebar_group_by: "status" },
        { renamingSessionId: "scratch", onRenameSession, onStartRename },
      );
      const input = mounted.target.querySelector<HTMLInputElement>(
        "input[aria-label='Rename session']",
      )!;
      expect(input.value).toBe("scratch");

      input.value = "  renamed  ";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      expect(onRenameSession).toHaveBeenCalledWith("scratch", "renamed");
      expect(onStartRename).toHaveBeenCalledWith("");
    },
  );

  it("mounts session indicator and meta plugin slots on session cards", async () => {
    const plugin = { id: "ci", name: "CI", source_kind: "builtin" } as PluginInventory;
    const slot = (placement: PluginUiContribution["placement"]) => ({
      plugin,
      contribution: { id: placement, label: placement, placement } as PluginUiContribution,
    });
    mounted = await mountSidebar(
      { sidebar_group_by: "status" },
      {
        sessionIndicatorContributions: [slot("session.indicator")],
        sessionMetaContributions: [slot("session.meta")],
      },
    );

    const card = rowOf(mounted.target, "scratch");
    expect(
      card
        .querySelector('[data-plugin-session-indicator="scratch"] [data-plugin-ui-contribution]')
        ?.getAttribute("data-plugin-ui-contribution"),
    ).toBe("ci:session.indicator");
    expect(
      card
        .querySelector('[data-plugin-session-meta="scratch"] [data-plugin-ui-contribution]')
        ?.getAttribute("data-plugin-ui-contribution"),
    ).toBe("ci:session.meta");
  });

  it("leaves Enter to a keyboard-focused meta pill, and gives Enter back to the row after a move", async () => {
    const onSelectSession = vi.fn();
    const onOpenPluginSessionPanel = vi.fn();
    const contribution = (id: string, placement: PluginUiContribution["placement"]) =>
      ({ id, label: id, placement }) as PluginUiContribution;
    const plugin = {
      id: "github",
      name: "GitHub",
      source_kind: "local",
      host_api_version: "planeai.plugin-host.v5",
      ui_contributions: [
        contribution("pill", "session.meta"),
        contribution("panel", "session.panel"),
      ],
    } as PluginInventory;
    mounted = await mountSidebar(
      { sidebar_group_by: "status" },
      {
        onSelectSession,
        onOpenPluginSessionPanel,
        sessionMetaContributions: [{ plugin, contribution: plugin.ui_contributions[0] }],
      },
    );
    focusSidebar();
    flushSync();
    const rows = navLabels(mounted.target);
    setSelectedIndex(rows.indexOf("scratch"));
    flushSync();
    const pill = rowOf(mounted.target, "scratch").querySelector<HTMLButtonElement>(
      '[data-plugin-session-meta="scratch"] button',
    )!;
    pill.focus();
    vi.spyOn(pill, "matches").mockImplementation((selector) => selector === ":focus-visible");
    const pressOn = (target: Element, key: string) => {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      target.dispatchEvent(event);
      flushSync();
      return event;
    };

    const enter = pressOn(pill, "Enter");
    expect(enter.defaultPrevented, "the browser activates the pill").toBe(false);
    expect(onSelectSession, "Enter does not select the highlighted row").not.toHaveBeenCalled();

    pill.click();
    expect(onOpenPluginSessionPanel).toHaveBeenCalledWith("scratch", "github", "panel");
    expect(onSelectSession, "the pill's click does not select its row").not.toHaveBeenCalled();

    pressOn(pill, "k");
    expect(document.activeElement, "moving the highlight releases the pill").not.toBe(pill);
    pressOn(document.body, "Enter");
    expect(onSelectSession).toHaveBeenCalledTimes(1);
  });

  it("splits the top rows into project folders when grouped by project", async () => {
    mounted = await mountSidebar({});

    expect(navLabels(mounted.target).slice(0, 5)).toEqual([
      "[alpha 0]",
      "[beta-service 2]",
      "scratch",
      "Title B-2",
      "[Running 2]",
    ]);
    expect(mounted.target.querySelector("[data-folder-urgency]")).toBeNull();

    navButtons(mounted.target)[1].click();
    flushSync();
    const dot = mounted.target.querySelector<HTMLElement>("[data-folder-urgency]")!;
    expect(dot.dataset.folderUrgency).toBe("review");
    expect(dot.getAttribute("role")).toBe("img");
    expect(dot.getAttribute("aria-label")).toBe("Needs review");
    expect(dot.title).toBe("Needs review");
  });

  it("keeps the Classic rows when the layout is classic", async () => {
    mounted = await mountSidebar({ sidebar_layout: "classic", sidebar_group_by: "status" });

    expect(
      navButtons(mounted.target)
        .slice(0, 3)
        .map((b) => b.textContent!.replace(/\s+/g, " ").trim()),
    ).toEqual(["Sessions 1", "beta-service scratch", "Running 2"]);
  });

  it("moves a review task to Done from its hover action", async () => {
    mounted = await mountSidebar({ sidebar_group_by: "status" });

    clickAction(mounted.target, "Title B-2", "Mark done");

    await vi.waitFor(() => expect(navLabels(mounted!.target).at(-1)).toBe("[Done 2]"));
    expect(taskMove).not.toHaveBeenCalled();
    await commitPendingDone();
    expect(taskMove).toHaveBeenCalledWith("B-2", "done", beta.path);
  });

  it("animates a row leaving its list, unless the user prefers reduced motion", async () => {
    const animate = vi.spyOn(Element.prototype, "animate");
    const matchMedia = window.matchMedia;
    const moveToDone = async (title: string, doneCount: number) => {
      mounted = await mountSidebar({ sidebar_group_by: "status" });
      animate.mockClear();
      clickAction(mounted.target, title, "Mark done");
      await vi.waitFor(() => expect(navLabels(mounted!.target).at(-1)).toBe(`[Done ${doneCount}]`));
      await commitPendingDone();
      unmount(mounted.component);
      mounted.target.remove();
      mounted = undefined;
      return animate.mock.calls.length > 0;
    };

    const animated = await moveToDone("Title B-2", 2);
    window.matchMedia = (query: string) =>
      ({
        ...matchMedia(query),
        matches: query === "(prefers-reduced-motion: reduce)",
      }) as MediaQueryList;
    try {
      expect([animated, await moveToDone("Title B-1", 3)]).toEqual([true, false]);
    } finally {
      window.matchMedia = matchMedia;
      animate.mockRestore();
    }
  });

  it("archives an unlinked session from its Done action", async () => {
    const onArchiveSession = vi.fn();
    mounted = await mountSidebar({ sidebar_group_by: "status" }, { onArchiveSession });

    clickAction(mounted.target, "scratch", "Archive session");

    await vi.waitFor(() => expect(onArchiveSession).toHaveBeenCalledOnce());
    expect(onArchiveSession.mock.calls[0][0].id).toBe("scratch");
    expect(taskMove).not.toHaveBeenCalled();
  });

  it("reopens a done task and starts a to-do task from their hover actions", async () => {
    const onPickTask = vi.fn();
    mounted = await mountSidebar({ sidebar_group_by: "status" }, { onPickTask });
    navButtons(mounted.target).at(-1)!.click();
    flushSync();

    clickAction(mounted.target, "Title A-2", "Start session");
    clickAction(mounted.target, "Title A-3", "Reopen");

    expect(onPickTask.mock.calls.map(([t, path]) => [t.key, path])).toEqual([["A-2", alpha.path]]);
    expect(taskMove).toHaveBeenCalledWith("A-3", "todo", alpha.path);
    await vi.waitFor(() => expect(navLabels(mounted!.target)).toContain("[To do 2]"));
  });

  it("never selects the row when one of its actions is clicked", async () => {
    const onSelectTask = vi.fn();
    mounted = await mountSidebar({ sidebar_group_by: "status" }, { onSelectTask });

    clickAction(mounted.target, "Title A-2", "Start session");

    expect(onSelectTask).not.toHaveBeenCalled();
  });

  it("pages the Done shelf 25 rows at a time", async () => {
    tasksByPath[alpha.path] = Array.from({ length: 30 }, (_, i) => task(`D-${i + 1}`, "done"));
    tasksByPath[beta.path] = [];
    sessionList.mockResolvedValue([]);
    mounted = await mountSidebar({ sidebar_group_by: "status" });

    navButtons(mounted.target)[0].click();
    flushSync();
    const labels = navLabels(mounted.target);
    expect([labels.length, labels[1], labels.at(-2), labels.at(-1)]).toEqual([
      27,
      "Title D-1",
      "Title D-25",
      "Show 5 more",
    ]);

    navButtons(mounted.target).at(-1)!.click();
    flushSync();
    expect(navLabels(mounted.target).slice(-2)).toEqual(["Title D-29", "Title D-30"]);
    expect(navLabels(mounted.target)).toHaveLength(31);
  });

  it.each([
    { keys: "hjkl", down: "j", left: "h", right: "l" },
    { keys: "arrows", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight" },
  ])(
    "walks rows and shelf headers with $keys and pages Done with Enter",
    async ({ down, left, right }) => {
      tasksByPath[alpha.path] = [
        task("A-1", "todo"),
        ...Array.from({ length: 26 }, (_, i) => task(`D-${i + 1}`, "done")),
      ];
      tasksByPath[beta.path] = [];
      sessionList.mockResolvedValue([]);
      mounted = await mountSidebar({ sidebar_group_by: "status" });
      focusSidebar();
      flushSync();
      setSelectedIndex(0);
      flushSync();
      const selected = () => navLabels(mounted!.target)[getSelectedIndex()];

      press(down);
      expect(selected()).toBe("Title A-1");
      press(down);
      expect(selected()).toBe("[Done 26]");
      press(right);
      for (let i = 0; i < 26; i++) press(down);
      expect(selected()).toBe("Show 1 more");

      press("Enter");
      expect(selected()).toBe("Title D-26");
      press(left);
      expect(selected()).toBe("[Done 26]");
      press(left);
      expect(navLabels(mounted.target)).toEqual(["[To do 1]", "Title A-1", "[Done 26]"]);
    },
  );

  it("orders Mod+1-9 and Mod+{/} targets by the rows the sidebar shows, skipping folded shelves", async () => {
    tasksByPath[alpha.path] = [
      task("A-1", "in_progress"),
      task("A-2", "todo"),
      ...Array.from({ length: 26 }, (_, i) => task(`D-${i + 1}`, "done")),
    ];
    tasksByPath[beta.path] = [task("B-1", "in_review")];
    sessionList.mockResolvedValue([]);
    mounted = await mountSidebar({ sidebar_group_by: "status" });
    const header = (prefix: string) =>
      navButtons(mounted!.target).find((b) => labelOf(b).startsWith(`[${prefix}`))!;
    const done = (from: number, to: number) =>
      Array.from({ length: to - from + 1 }, (_, i) => `task:alpha:D-${from + i}`);

    expect(getVisibleNavigationOrder()).toEqual([
      "task:beta:B-1",
      "task:alpha:A-1",
      "task:alpha:A-2",
    ]);

    header("Done").click();
    flushSync();
    expect(getVisibleNavigationOrder()).toEqual([
      "task:beta:B-1",
      "task:alpha:A-1",
      "task:alpha:A-2",
      ...done(1, 25),
    ]);

    navButtons(mounted.target).at(-1)!.click();
    header("To do").click();
    flushSync();
    expect(getVisibleNavigationOrder()).toEqual([
      "task:beta:B-1",
      "task:alpha:A-1",
      ...done(1, 26),
    ]);
  });
});
