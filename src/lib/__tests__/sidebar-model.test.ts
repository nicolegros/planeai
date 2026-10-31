import { describe, expect, it } from "vitest";
import {
  buildSidebarModel,
  projectLabelsApply,
  resolveSidebarLayout,
  sidebarNavItemKey,
  sidebarNavItems,
  sidebarNavigationOrder,
  taskHeaderNavKeys,
  type SidebarModelInput,
  type SidebarRow,
  type SidebarSection,
} from "../sidebar-model";
import type { Project, Session, TaskItem } from "../types";

function project(id: string, overrides: Partial<Project> = {}): Project {
  return { id, name: id, path: `/repos/${id}`, hidden: false, path_missing: false, ...overrides };
}

function task(
  key: string,
  status: string,
  priority = 0,
  overrides: Partial<TaskItem> = {},
): TaskItem {
  return {
    key,
    title: key,
    status,
    description: "",
    priority,
    blocked_by: [],
    tags: [],
    parent_key: null,
    url: null,
    base_branch: "main",
    ...overrides,
  };
}

function session(
  id: string,
  projectId: string,
  taskKey: string | null = null,
  taskProjectId: string | null = null,
): Session {
  return {
    id,
    project_id: projectId,
    name: id,
    tmux_name: null,
    branch: id,
    status: "active",
    created_at: "2026-01-01T00:00:00Z",
    worktree_path: null,
    provider: null,
    backend: "local",
    base_branch: null,
    task_key: taskKey,
    task_project_id: taskProjectId,
  };
}

function input(overrides: Partial<SidebarModelInput> = {}): SidebarModelInput {
  return {
    layout: "classic",
    groupBy: "project",
    projects: [],
    sessions: [],
    tasksByProject: {},
    hideDoneTasks: false,
    hideEmptyProjects: false,
    ...overrides,
  };
}

function keysOf(sections: SidebarSection[]): string[] {
  return sections.map((s) => s.key);
}

const alpha = project("alpha");
const beta = project("beta");

describe("buildSidebarModel - project mode", () => {
  it("groups tasks by status inside each project, in status order", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha],
        tasksByProject: {
          [alpha.path]: [
            task("A-1", "done"),
            task("A-2", "todo"),
            task("A-3", "in_review"),
            task("A-4", "in_progress"),
          ],
        },
      }),
    );

    expect(keysOf(sections)).toEqual(["project:alpha"]);
    const section = sections[0];
    if (section.kind !== "project") throw new Error("expected project section");
    expect(section.groups.map((g) => g.status)).toEqual([
      "in_progress",
      "in_review",
      "todo",
      "done",
    ]);
    expect(section.groups.map((g) => g.key)).toEqual([
      "project-status:alpha:in_progress",
      "project-status:alpha:in_review",
      "project-status:alpha:todo",
      "project-status:alpha:done",
    ]);
  });

  it("buckets unknown statuses into todo", () => {
    const sections = buildSidebarModel(
      input({ projects: [alpha], tasksByProject: { [alpha.path]: [task("A-1", "blocked")] } }),
    );
    const section = sections[0];
    if (section.kind !== "project") throw new Error("expected project section");
    expect(section.groups.map((g) => [g.status, g.rows.map((r) => r.task.key)])).toEqual([
      ["todo", ["A-1"]],
    ]);
  });

  it("places unlinked sessions in their project", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, beta],
        sessions: [
          session("s1", "alpha"),
          session("s2", "beta", "B-1"),
          session("s3", "beta", "GONE-1"),
          session("s4", "alpha"),
        ],
        tasksByProject: { [beta.path]: [task("B-1", "todo")] },
      }),
    );
    const [a, b] = sections;
    if (a.kind !== "project" || b.kind !== "project") throw new Error("expected project sections");
    expect(a.orphans.map((r) => r.session.id)).toEqual(["s1", "s4"]);
    expect(b.orphans.map((r) => r.session.id)).toEqual(["s3"]);
  });

  it("matches sessions to tasks by project and key when keys repeat across projects", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, beta],
        sessions: [
          session("on-beta", "beta", "X-1"),
          session("cross", "alpha", "Y-1", "beta"),
          session("stray", "alpha", "Y-1"),
        ],
        tasksByProject: {
          [alpha.path]: [task("X-1", "todo")],
          [beta.path]: [task("X-1", "todo"), task("Y-1", "todo")],
        },
      }),
    );
    const [a, b] = sections;
    if (a.kind !== "project" || b.kind !== "project") throw new Error("expected project sections");
    const linked = (section: typeof a) =>
      section.groups.flatMap((g) => g.rows.map((r) => [r.task.key, r.linkedSession?.id ?? null]));
    expect(linked(a)).toEqual([["X-1", null]]);
    expect(linked(b)).toEqual([
      ["X-1", "on-beta"],
      ["Y-1", "cross"],
    ]);
    // alpha has no Y-1 task, so a session linking Y-1 in its own project is unlinked.
    expect(a.orphans.map((r) => r.session.id)).toEqual(["stray"]);
  });

  it("collapses Done groups and empty projects by default", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, beta],
        tasksByProject: { [alpha.path]: [task("A-1", "done"), task("A-2", "todo")] },
      }),
    );
    const [a, b] = sections;
    if (a.kind !== "project" || b.kind !== "project") throw new Error("expected project sections");
    expect([a.defaultCollapsed, b.defaultCollapsed]).toEqual([false, true]);
    expect(a.groups.map((g) => [g.status, g.defaultCollapsed])).toEqual([
      ["todo", false],
      ["done", true],
    ]);
  });

  it("drops the done group when hideDoneTasks is on", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha],
        hideDoneTasks: true,
        tasksByProject: { [alpha.path]: [task("A-1", "done"), task("A-2", "todo")] },
      }),
    );
    const section = sections[0];
    if (section.kind !== "project") throw new Error("expected project section");
    expect(section.groups.map((g) => g.status)).toEqual(["todo"]);
  });

  it("skips hidden projects and, with hideEmptyProjects, projects with nothing visible", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, beta, project("gamma", { hidden: true })],
        hideEmptyProjects: true,
        hideDoneTasks: true,
        tasksByProject: { [alpha.path]: [task("A-1", "done")], [beta.path]: [task("B-1", "todo")] },
      }),
    );
    expect(keysOf(sections)).toEqual(["project:beta"]);
  });
});

describe("buildSidebarModel - status mode", () => {
  it("puts a sessions section first, then one flat section per status across projects", () => {
    const sections = buildSidebarModel(
      input({
        groupBy: "status",
        projects: [alpha, beta],
        sessions: [session("s1", "beta"), session("s2", "alpha")],
        tasksByProject: {
          [alpha.path]: [task("A-1", "in_progress"), task("A-2", "done")],
          [beta.path]: [task("B-1", "in_progress"), task("B-2", "in_review")],
        },
      }),
    );

    expect(keysOf(sections)).toEqual([
      "sessions",
      "status:in_progress",
      "status:in_review",
      "status:done",
    ]);
    const [sessions, running] = sections;
    if (sessions.kind !== "sessions" || running.kind !== "status")
      throw new Error("unexpected section kinds");
    expect(sessions.orphans.map((r) => [r.session.id, r.project.id])).toEqual([
      ["s2", "alpha"],
      ["s1", "beta"],
    ]);
    expect(running.group.rows.map((r) => [r.task.key, r.project.id])).toEqual([
      ["A-1", "alpha"],
      ["B-1", "beta"],
    ]);
  });

  it("omits the sessions section when there are no unlinked sessions", () => {
    const sections = buildSidebarModel(
      input({
        groupBy: "status",
        projects: [alpha],
        tasksByProject: { [alpha.path]: [task("A-1", "todo")] },
      }),
    );
    expect(keysOf(sections)).toEqual(["status:todo"]);
  });

  it("sorts by priority, then project sidebar order, then natural task key", () => {
    const sections = buildSidebarModel(
      input({
        groupBy: "status",
        projects: [beta, alpha],
        tasksByProject: {
          [alpha.path]: [task("A-10", "todo", 1), task("A-9", "todo", 1), task("A-1", "todo", 3)],
          [beta.path]: [task("B-2", "todo", 1), task("B-1", "todo", 0)],
        },
      }),
    );
    const todo = sections[0];
    if (todo.kind !== "status") throw new Error("expected status section");
    expect(todo.group.rows.map((r) => r.task.key)).toEqual(["A-1", "B-2", "A-9", "A-10", "B-1"]);
  });

  it("excludes hidden projects, ignores hideEmptyProjects and honors hideDoneTasks", () => {
    const sections = buildSidebarModel(
      input({
        groupBy: "status",
        projects: [alpha, project("hidden", { hidden: true })],
        sessions: [session("s1", "hidden")],
        hideEmptyProjects: true,
        hideDoneTasks: true,
        tasksByProject: {
          [alpha.path]: [task("A-1", "todo"), task("A-2", "done")],
          "/repos/hidden": [task("H-1", "todo")],
        },
      }),
    );
    expect(keysOf(sections)).toEqual(["status:todo"]);
    const todo = sections[0];
    if (todo.kind !== "status") throw new Error("expected status section");
    expect(todo.group.rows.map((r) => r.task.key)).toEqual(["A-1"]);
  });
});

describe("sidebarNavigationOrder", () => {
  it("excludes hidden projects", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, project("hidden", { hidden: true })],
        sessions: [session("s1", "alpha"), session("s2", "hidden")],
      }),
    );
    expect(sidebarNavigationOrder(sections)).toEqual(["s1"]);
  });

  it("follows project mode display order", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, beta],
        sessions: [session("s1", "alpha"), session("s2", "alpha")],
        tasksByProject: {
          [alpha.path]: [task("A-1", "todo"), task("A-2", "in_progress")],
          [beta.path]: [task("B-1", "todo")],
        },
      }),
    );
    expect(sidebarNavigationOrder(sections)).toEqual([
      "s1",
      "s2",
      "task:alpha:A-2",
      "task:alpha:A-1",
      "task:beta:B-1",
    ]);
  });

  it("follows status mode display order", () => {
    const sections = buildSidebarModel(
      input({
        groupBy: "status",
        projects: [alpha, beta],
        sessions: [session("s1", "beta"), session("s2", "alpha")],
        tasksByProject: {
          [alpha.path]: [task("A-1", "todo")],
          [beta.path]: [task("B-1", "in_progress")],
        },
      }),
    );
    expect(sidebarNavigationOrder(sections)).toEqual([
      "s2",
      "s1",
      "task:beta:B-1",
      "task:alpha:A-1",
    ]);
  });
});

describe("sidebarNavItems", () => {
  const sections = () =>
    buildSidebarModel(
      input({
        groupBy: "status",
        projects: [alpha],
        sessions: [session("s1", "alpha")],
        tasksByProject: { [alpha.path]: [task("A-1", "todo"), task("A-2", "done")] },
      }),
    );

  it("skips the rows of collapsed sections using their defaults", () => {
    const collapsed: Record<string, boolean> = { sessions: true };
    const items = sidebarNavItems(sections(), (key, fallback) => collapsed[key] ?? fallback);
    expect(items.map(sidebarNavItemKey)).toEqual([
      "group:sessions",
      "group:status:todo",
      "task:alpha:A-1",
      "group:status:done",
    ]);
  });

  it("lists every row when nothing is collapsed", () => {
    expect(sidebarNavItems(sections()).map(sidebarNavItemKey)).toEqual([
      "group:sessions",
      "orphan:s1",
      "group:status:todo",
      "task:alpha:A-1",
      "group:status:done",
      "task:alpha:A-2",
    ]);
  });
});

describe("resolveSidebarLayout", () => {
  it("defaults to cards", () => {
    expect([resolveSidebarLayout({}), resolveSidebarLayout({ sidebar_layout: "classic" })]).toEqual(
      ["cards", "classic"],
    );
  });
});

describe("projectLabelsApply", () => {
  it("applies everywhere except Classic grouped by project", () => {
    expect([
      projectLabelsApply({}),
      projectLabelsApply({ sidebar_layout: "cards", sidebar_group_by: "project" }),
      projectLabelsApply({ sidebar_layout: "classic", sidebar_group_by: "status" }),
      projectLabelsApply({ sidebar_layout: "classic", sidebar_group_by: "project" }),
    ]).toEqual([true, true, true, false]);
  });
});

const rowId = (row: SidebarRow) => (row.kind === "task" ? row.task.key : row.session.id);

function cardsShape(sections: SidebarSection[]) {
  return sections.map((section) => {
    switch (section.kind) {
      case "attention":
        return [section.key, section.rows.map(rowId)];
      case "folder":
      case "shelf":
        return [section.key, section.defaultCollapsed, section.rows.map(rowId)];
      default:
        throw new Error(`unexpected ${section.kind} section in Cards`);
    }
  });
}

describe("buildSidebarModel - cards layout", () => {
  const tasks = {
    [alpha.path]: [
      task("A-1", "in_progress"),
      task("A-2", "in_review"),
      task("A-3", "todo"),
      task("A-4", "done"),
      task("A-5", "in_review", 2),
    ],
    [beta.path]: [task("B-1", "in_review", 1), task("B-2", "todo", 3), task("B-3", "blocked")],
  };
  const sessions = [
    session("s1", "beta"),
    session("s2", "alpha"),
    session("linked", "alpha", "A-1"),
  ];

  it("grouped by status, puts unlinked sessions then review tasks on top and status shelves below", () => {
    const sections = buildSidebarModel(
      input({
        layout: "cards",
        groupBy: "status",
        projects: [alpha, beta],
        sessions,
        tasksByProject: tasks,
      }),
    );
    expect(cardsShape(sections)).toEqual([
      ["attention", ["s2", "s1", "A-5", "B-1", "A-2"]],
      ["shelf:in_progress", false, ["A-1"]],
      ["shelf:todo", false, ["B-2", "A-3", "B-3"]],
      ["shelf:done", true, ["A-4"]],
    ]);
  });

  it("grouped by project, splits only the top rows into project folders and keeps shelves global", () => {
    const gamma = project("gamma");
    const sections = buildSidebarModel(
      input({
        layout: "cards",
        projects: [alpha, beta, gamma],
        sessions,
        tasksByProject: { ...tasks, [gamma.path]: [task("G-1", "todo")] },
      }),
    );
    expect(cardsShape(sections)).toEqual([
      ["folder:alpha", false, ["s2", "A-5", "A-2"]],
      ["folder:beta", false, ["s1", "B-1"]],
      ["folder:gamma", true, []],
      ["shelf:in_progress", false, ["A-1"]],
      ["shelf:todo", false, ["B-2", "A-3", "B-3", "G-1"]],
      ["shelf:done", true, ["A-4"]],
    ]);
  });

  it("omits empty shelves and the top area when nothing needs attention", () => {
    const sections = buildSidebarModel(
      input({
        layout: "cards",
        groupBy: "status",
        projects: [alpha],
        tasksByProject: { [alpha.path]: [task("A-1", "todo")] },
      }),
    );
    expect(cardsShape(sections)).toEqual([["shelf:todo", false, ["A-1"]]]);
  });

  it("drops the Done shelf with hideDoneTasks and skips hidden and empty projects", () => {
    const sections = buildSidebarModel(
      input({
        layout: "cards",
        projects: [alpha, beta, project("hidden", { hidden: true })],
        sessions: [session("s-hidden", "hidden")],
        hideDoneTasks: true,
        hideEmptyProjects: true,
        tasksByProject: {
          [alpha.path]: [task("A-1", "done")],
          [beta.path]: [task("B-1", "todo")],
          "/repos/hidden": [task("H-1", "in_review")],
        },
      }),
    );
    expect(cardsShape(sections)).toEqual([
      ["folder:beta", true, []],
      ["shelf:todo", false, ["B-1"]],
    ]);
  });
});

describe("sidebarNavItems - cards layout", () => {
  const doneTasks = Array.from({ length: 27 }, (_, i) => task(`D-${i + 1}`, "done"));
  const sections = () =>
    buildSidebarModel(
      input({
        layout: "cards",
        projects: [alpha],
        sessions: [session("s1", "alpha")],
        tasksByProject: {
          [alpha.path]: [task("A-1", "in_review"), task("A-2", "todo"), ...doneTasks],
        },
      }),
    );
  const expanded = () => false;

  it("lists folder rows, shelf headers and the first page of Done with a Show more row", () => {
    const keys = sidebarNavItems(sections(), expanded, () => 1).map(sidebarNavItemKey);
    expect(keys.slice(0, 7)).toEqual([
      "folder:alpha",
      "orphan:s1",
      "task:alpha:A-1",
      "group:shelf:todo",
      "task:alpha:A-2",
      "group:shelf:done",
      "task:alpha:D-1",
    ]);
    expect(keys.slice(-2)).toEqual(["task:alpha:D-25", "more:shelf:done"]);
    expect(keys).toHaveLength(32);
    expect(sidebarNavItems(sections(), expanded, () => 1).at(-1)).toEqual({
      type: "show_more",
      shelfKey: "shelf:done",
    });
  });

  it("shows every Done row once enough pages are open", () => {
    const keys = sidebarNavItems(sections(), expanded, () => 2).map(sidebarNavItemKey);
    expect(keys.slice(-2)).toEqual(["task:alpha:D-26", "task:alpha:D-27"]);
    expect(keys).toHaveLength(33);
  });

  it("skips the rows of collapsed folders and shelves, with Done collapsed by default", () => {
    const collapsed: Record<string, boolean> = { "folder:alpha": true };
    const keys = sidebarNavItems(sections(), (key, fallback) => collapsed[key] ?? fallback).map(
      sidebarNavItemKey,
    );
    expect(keys).toEqual([
      "folder:alpha",
      "group:shelf:todo",
      "task:alpha:A-2",
      "group:shelf:done",
    ]);
  });
});

describe("sidebarNavigationOrder - cards layout", () => {
  it("follows the Cards display order, including every Done row", () => {
    const sections = buildSidebarModel(
      input({
        layout: "cards",
        groupBy: "status",
        projects: [alpha, beta],
        sessions: [session("s1", "beta"), session("linked", "alpha", "A-3")],
        tasksByProject: {
          [alpha.path]: [task("A-1", "done"), task("A-2", "todo"), task("A-3", "in_progress")],
          [beta.path]: [task("B-1", "in_review")],
        },
      }),
    );
    expect(sidebarNavigationOrder(sections)).toEqual([
      "s1",
      "task:beta:B-1",
      "task:alpha:A-3",
      "task:alpha:A-2",
      "task:alpha:A-1",
    ]);
  });
});

describe("taskHeaderNavKeys", () => {
  const tasksByProject = {
    [alpha.path]: [task("A-1", "in_review"), task("A-2", "todo")],
  };

  it("names the enclosing group, then project, in Classic", () => {
    const sections = buildSidebarModel(input({ projects: [alpha], tasksByProject }));
    expect(taskHeaderNavKeys(sections, "alpha", "A-2")).toEqual([
      "group:project-status:alpha:todo",
      "project:alpha",
    ]);
  });

  it("names the folder or shelf in Cards, and nothing for an unknown task", () => {
    const sections = buildSidebarModel(
      input({ layout: "cards", projects: [alpha], tasksByProject }),
    );
    expect([
      taskHeaderNavKeys(sections, "alpha", "A-1"),
      taskHeaderNavKeys(sections, "alpha", "A-2"),
      taskHeaderNavKeys(sections, "alpha", "A-9"),
    ]).toEqual([["folder:alpha"], ["group:shelf:todo"], []]);
  });
});
