import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import type { Project, Session, TaskItem } from "../../lib/types";

const { configGet, configUpdate, projectList, sessionList, taskListAll, taskMove } = vi.hoisted(
  () => ({
    configGet: vi.fn(),
    configUpdate: vi.fn(),
    projectList: vi.fn(),
    sessionList: vi.fn(),
    taskListAll: vi.fn(),
    taskMove: vi.fn(),
  }),
);

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

vi.mock("../../lib/toasts.svelte", () => ({ showToast: vi.fn() }));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ onCloseRequested: vi.fn().mockResolvedValue(() => {}) })),
}));

import UnifiedSidebar from "../UnifiedSidebar.svelte";
import { focusSidebar, focusTerminal } from "../../lib/focus.svelte";
import { loadProjects } from "../../lib/project-store.svelte";
import { _resetForTests, loadSessions } from "../../lib/session-orchestrator.svelte";
import { getSelectedIndex, setSelectedIndex } from "../../lib/sidebar-nav.svelte";
import { loadTasks } from "../../lib/task-store.svelte";
import { loadSettings } from "../../lib/settings.svelte";

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
  return (button.querySelector("span.text-\\[13px\\]") ?? button).textContent!.trim();
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
    expect(
      mounted.target.querySelector<HTMLElement>("[data-folder-urgency]")!.dataset.folderUrgency,
    ).toBe("review");
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

    expect(taskMove).toHaveBeenCalledWith("B-2", "done", beta.path);
    await vi.waitFor(() => expect(navLabels(mounted!.target).at(-1)).toBe("[Done 2]"));
  });

  it("animates a row leaving its list, unless the user prefers reduced motion", async () => {
    const animate = vi.spyOn(Element.prototype, "animate");
    const matchMedia = window.matchMedia;
    const moveToDone = async (title: string, doneCount: number) => {
      mounted = await mountSidebar({ sidebar_group_by: "status" });
      animate.mockClear();
      clickAction(mounted.target, title, "Mark done");
      await vi.waitFor(() => expect(navLabels(mounted!.target).at(-1)).toBe(`[Done ${doneCount}]`));
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

  it("walks rows and shelf headers from the keyboard and pages Done with Enter", async () => {
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

    press("j");
    expect(selected()).toBe("Title A-1");
    press("j");
    expect(selected()).toBe("[Done 26]");
    press("l");
    for (let i = 0; i < 26; i++) press("j");
    expect(selected()).toBe("Show 1 more");

    press("Enter");
    expect(selected()).toBe("Title D-26");
    press("h");
    expect(selected()).toBe("[Done 26]");
    press("h");
    expect(navLabels(mounted.target)).toEqual(["[To do 1]", "Title A-1", "[Done 26]"]);
  });
});
