import { describe, it, expect } from "vitest";
import type { Session, TaskItem, Project } from "../types";

// Test the derivation logic used by UnifiedSidebar

function makeProject(id: string, name: string, path: string): Project {
  return { id, name, path };
}

function makeSession(id: string, projectId: string, taskKey: string | null = null): Session {
  return {
    id,
    project_id: projectId,
    name: "",
    tmux_name: null,
    branch: "main",
    status: "active",
    created_at: "",
    worktree_path: null,
    provider: null,
    backend: "direct",
    tab_count: 1,
    base_branch: null,
    task_key: taskKey,
    pr_url: null,
    pr_state: null,
  };
}

function makeTask(
  key: string,
  status: string,
  title = "task",
  parent_key: string | null = null,
): TaskItem {
  return {
    key,
    title,
    status,
    description: "",
    priority: 0,
    blocked_by: [],
    tags: [],
    parent_key,
    url: null,
    base_branch: "main",
  };
}

function isParentTask(task: TaskItem, allTasks: TaskItem[]): boolean {
  return allTasks.some((t) => t.parent_key === task.key);
}

function getOrphanSessions(sessions: Session[], allTaskKeys: Set<string>): Session[] {
  return sessions.filter((s) => !s.task_key || !allTaskKeys.has(s.task_key));
}

function groupByStatus(items: TaskItem[]): Record<string, TaskItem[]> {
  const statusOrder = ["in_progress", "in_review", "todo", "done"];
  const groups: Record<string, TaskItem[]> = {};
  for (const s of statusOrder) groups[s] = [];
  for (const t of items) (groups[t.status] ?? (groups["todo"] ??= [])).push(t);
  for (const s of statusOrder) groups[s]?.sort((a, b) => b.priority - a.priority);
  return groups;
}

describe("unified sidebar logic", () => {
  describe("orphan detection", () => {
    it("session with no task_key is orphan", () => {
      const sessions = [makeSession("s1", "p1", null)];
      const orphans = getOrphanSessions(sessions, new Set(["PLA-1"]));
      expect(orphans).toHaveLength(1);
      expect(orphans[0].id).toBe("s1");
    });

    it("session with task_key matching a task is NOT orphan", () => {
      const sessions = [makeSession("s1", "p1", "PLA-1")];
      const orphans = getOrphanSessions(sessions, new Set(["PLA-1"]));
      expect(orphans).toHaveLength(0);
    });

    it("session with task_key not in task list IS orphan", () => {
      const sessions = [makeSession("s1", "p1", "PLA-99")];
      const orphans = getOrphanSessions(sessions, new Set(["PLA-1"]));
      expect(orphans).toHaveLength(1);
    });

    it("mixed sessions correctly partitioned", () => {
      const sessions = [
        makeSession("s1", "p1", "PLA-1"),
        makeSession("s2", "p1", null),
        makeSession("s3", "p1", "PLA-2"),
        makeSession("s4", "p1", "PLA-99"),
      ];
      const orphans = getOrphanSessions(sessions, new Set(["PLA-1", "PLA-2"]));
      expect(orphans.map((s) => s.id)).toEqual(["s2", "s4"]);
    });
  });

  describe("task grouping by status", () => {
    it("groups tasks into status buckets", () => {
      const tasks = [
        makeTask("T-1", "todo"),
        makeTask("T-2", "in_progress"),
        makeTask("T-3", "done"),
        makeTask("T-4", "todo"),
      ];
      const groups = groupByStatus(tasks);
      expect(groups["todo"]).toHaveLength(2);
      expect(groups["in_progress"]).toHaveLength(1);
      expect(groups["done"]).toHaveLength(1);
      expect(groups["in_review"]).toHaveLength(0);
    });

    it("sorts by priority descending within group", () => {
      const tasks = [
        { ...makeTask("T-1", "todo"), priority: 1 },
        { ...makeTask("T-2", "todo"), priority: 3 },
        { ...makeTask("T-3", "todo"), priority: 2 },
      ];
      const groups = groupByStatus(tasks);
      expect(groups["todo"].map((t) => t.key)).toEqual(["T-2", "T-3", "T-1"]);
    });

    it("handles empty input", () => {
      const groups = groupByStatus([]);
      expect(groups["todo"]).toHaveLength(0);
      expect(groups["in_progress"]).toHaveLength(0);
    });
  });

  describe("parent task detection", () => {
    it("task with subtasks is a parent", () => {
      const tasks = [
        makeTask("PLA-1", "todo", "parent"),
        makeTask("PLA-2", "todo", "child", "PLA-1"),
      ];
      expect(isParentTask(tasks[0], tasks)).toBe(true);
    });

    it("task without subtasks is not a parent", () => {
      const tasks = [makeTask("PLA-1", "todo", "standalone"), makeTask("PLA-2", "todo", "other")];
      expect(isParentTask(tasks[0], tasks)).toBe(false);
    });

    it("child task is not a parent", () => {
      const tasks = [
        makeTask("PLA-1", "todo", "parent"),
        makeTask("PLA-2", "todo", "child", "PLA-1"),
      ];
      expect(isParentTask(tasks[1], tasks)).toBe(false);
    });
  });

  describe("auto-collapse empty projects", () => {
    function isProjectCollapsed(
      project: Project,
      collapsedSections: Record<string, boolean>,
      orphansByProject: { project: Project; sessions: Session[] }[],
      tasksByProject: Record<string, TaskItem[]>,
    ): boolean {
      const key = `project:${project.id}`;
      return collapsedSections[key] ?? (
        (orphansByProject.find((g) => g.project.id === project.id)?.sessions ?? []).length === 0 &&
        (tasksByProject[project.path] ?? []).length === 0
      );
    }

    it("empty project defaults to collapsed", () => {
      const p = makeProject("p1", "empty", "/empty");
      expect(isProjectCollapsed(p, {}, [], {})).toBe(true);
    });

    it("project with orphan sessions defaults to expanded", () => {
      const p = makeProject("p1", "proj", "/proj");
      const orphans = [{ project: p, sessions: [makeSession("s1", "p1")] }];
      expect(isProjectCollapsed(p, {}, orphans, {})).toBe(false);
    });

    it("project with tasks defaults to expanded", () => {
      const p = makeProject("p1", "proj", "/proj");
      expect(isProjectCollapsed(p, {}, [], { "/proj": [makeTask("T-1", "todo")] })).toBe(false);
    });

    it("respects explicit user toggle to expanded", () => {
      const p = makeProject("p1", "empty", "/empty");
      expect(isProjectCollapsed(p, { "project:p1": false }, [], {})).toBe(false);
    });

    it("respects explicit user toggle to collapsed", () => {
      const p = makeProject("p1", "proj", "/proj");
      const orphans = [{ project: p, sessions: [makeSession("s1", "p1")] }];
      expect(isProjectCollapsed(p, { "project:p1": true }, orphans, {})).toBe(true);
    });
  });

  describe("flat nav ordering", () => {
    it("includes project_header, orphans, status_header, and tasks in order", () => {
      const projects = [makeProject("p1", "proj", "/path")];
      const sessions = [makeSession("s1", "p1", null), makeSession("s2", "p1", "T-1")];
      const tasks = [makeTask("T-1", "in_progress")];
      const allTaskKeys = new Set(tasks.map((t) => t.key));
      const orphans = getOrphanSessions(sessions, allTaskKeys);

      // Simulating the flatNav construction (new format with headers)
      type NavItem =
        | { type: "project_header"; id: string }
        | { type: "orphan"; id: string }
        | { type: "status_header"; status: string }
        | { type: "task"; key: string };
      const flatNav: NavItem[] = [];
      flatNav.push({ type: "project_header", id: "p1" });
      for (const s of orphans.filter((s) => s.project_id === "p1"))
        flatNav.push({ type: "orphan", id: s.id });
      flatNav.push({ type: "status_header", status: "in_progress" });
      for (const t of tasks) flatNav.push({ type: "task", key: t.key });

      expect(flatNav[0]).toEqual({ type: "project_header", id: "p1" });
      expect(flatNav[1]).toEqual({ type: "orphan", id: "s1" });
      expect(flatNav[2]).toEqual({ type: "status_header", status: "in_progress" });
      expect(flatNav[3]).toEqual({ type: "task", key: "T-1" });
    });

    it("collapsed project only shows project_header", () => {
      type NavItem = { type: "project_header"; id: string } | { type: "task"; key: string };
      const collapsed = new Set(["project:p1"]);
      const flatNav: NavItem[] = [];
      flatNav.push({ type: "project_header", id: "p1" });
      if (!collapsed.has("project:p1")) {
        flatNav.push({ type: "task", key: "T-1" });
      }
      expect(flatNav).toHaveLength(1);
      expect(flatNav[0]).toEqual({ type: "project_header", id: "p1" });
    });
  });
});
