import { describe, expect, it } from "vitest";
import {
  buildSidebarModel,
  isCollapsedByDefault,
  sidebarNavigationOrder,
  type SidebarModelInput,
  type SidebarSection,
} from "../sidebar-model";
import type { LoopRunSummary, LoopSessionItem, Project, Session, TaskItem } from "../types";

function project(id: string, overrides: Partial<Project> = {}): Project {
  return { id, name: id, path: `/repos/${id}`, hidden: false, ...overrides };
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

function session(id: string, projectId: string, taskKey: string | null = null): Session {
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
    task_project_id: null,
  };
}

function loop(id: string, projectId: string): LoopRunSummary {
  return { id, project_id: projectId } as LoopRunSummary;
}

function input(overrides: Partial<SidebarModelInput> = {}): SidebarModelInput {
  return {
    groupBy: "project",
    projects: [],
    sessions: [],
    tasksByProject: {},
    loopsByProject: {},
    loopSessionIds: new Set(),
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

  it("places loops and unlinked sessions in their project", () => {
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
        loopsByProject: { beta: [loop("l1", "beta")] },
        loopSessionIds: new Set(["s4"]),
      }),
    );
    const [a, b] = sections;
    if (a.kind !== "project" || b.kind !== "project") throw new Error("expected project sections");
    expect(a.orphans.map((r) => r.session.id)).toEqual(["s1"]);
    expect(b.orphans.map((r) => r.session.id)).toEqual(["s3"]);
    expect(b.loops.map((r) => r.loop.id)).toEqual(["l1"]);
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
        loopsByProject: { beta: [loop("l1", "beta")] },
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
    expect(sessions.loops.map((r) => [r.loop.id, r.project.id])).toEqual([["l1", "beta"]]);
    expect(sessions.orphans.map((r) => [r.session.id, r.project.id])).toEqual([
      ["s2", "alpha"],
      ["s1", "beta"],
    ]);
    expect(running.group.rows.map((r) => [r.task.key, r.project.id])).toEqual([
      ["A-1", "alpha"],
      ["B-1", "beta"],
    ]);
  });

  it("omits the sessions section when there are no loops or unlinked sessions", () => {
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

describe("isCollapsedByDefault", () => {
  it("collapses done groups in both modes and nothing else", () => {
    expect(isCollapsedByDefault("status:done")).toBe(true);
    expect(isCollapsedByDefault("project-status:alpha:done")).toBe(true);
    expect(isCollapsedByDefault("status:todo")).toBe(false);
    expect(isCollapsedByDefault("project-status:alpha:in_progress")).toBe(false);
    expect(isCollapsedByDefault("sessions")).toBe(false);
    expect(isCollapsedByDefault("loop:done")).toBe(false);
  });
});

describe("sidebarNavigationOrder", () => {
  const loopSessions: Record<string, LoopSessionItem[]> = {
    l1: [{ session_id: "ls1" } as LoopSessionItem],
  };

  it("follows project mode display order", () => {
    const sections = buildSidebarModel(
      input({
        projects: [alpha, beta],
        sessions: [session("s1", "alpha"), session("ls1", "alpha")],
        loopsByProject: { alpha: [loop("l1", "alpha")] },
        loopSessionIds: new Set(["ls1"]),
        tasksByProject: {
          [alpha.path]: [task("A-1", "todo"), task("A-2", "in_progress")],
          [beta.path]: [task("B-1", "todo")],
        },
      }),
    );
    expect(sidebarNavigationOrder(sections, loopSessions)).toEqual([
      "loop:l1",
      "ls1",
      "s1",
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
        sessions: [session("s1", "beta"), session("ls1", "alpha")],
        loopsByProject: { alpha: [loop("l1", "alpha")] },
        loopSessionIds: new Set(["ls1"]),
        tasksByProject: {
          [alpha.path]: [task("A-1", "todo")],
          [beta.path]: [task("B-1", "in_progress")],
        },
      }),
    );
    expect(sidebarNavigationOrder(sections, loopSessions)).toEqual([
      "loop:l1",
      "ls1",
      "s1",
      "task:beta:B-1",
      "task:alpha:A-1",
    ]);
  });
});
