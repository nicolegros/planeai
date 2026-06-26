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
      return (
        collapsedSections[key] ??
        ((orphansByProject.find((g) => g.project.id === project.id)?.sessions ?? []).length === 0 &&
          (tasksByProject[project.path] ?? []).length === 0)
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
    // Replicates the flatNavIndex Map logic used by UnifiedSidebar for O(1) lookups
    type NavItem =
      | { type: "project_header"; project: { id: string } }
      | { type: "orphan"; session: { id: string } }
      | { type: "status_header"; projectPath: string; status: string }
      | { type: "task"; task: { key: string }; projectPath: string };

    function buildFlatNavIndex(flatNav: NavItem[]): Map<string, number> {
      const map = new Map<string, number>();
      flatNav.forEach((item, i) => {
        if (item.type === "project_header") map.set(`project:${item.project.id}`, i);
        else if (item.type === "orphan") map.set(`orphan:${item.session.id}`, i);
        else if (item.type === "status_header")
          map.set(`status:${item.projectPath}:${item.status}`, i);
        else if (item.type === "task") map.set(`task:${item.task.key}`, i);
      });
      return map;
    }

    it("includes project_header, orphans, status_header, and tasks in order", () => {
      const _projects = [makeProject("p1", "proj", "/path")];
      const sessions = [makeSession("s1", "p1", null), makeSession("s2", "p1", "T-1")];
      const tasks = [makeTask("T-1", "in_progress")];
      const allTaskKeys = new Set(tasks.map((t) => t.key));
      const orphans = getOrphanSessions(sessions, allTaskKeys);

      const flatNav: NavItem[] = [];
      flatNav.push({ type: "project_header", project: { id: "p1" } });
      for (const s of orphans.filter((s) => s.project_id === "p1"))
        flatNav.push({ type: "orphan", session: { id: s.id } });
      flatNav.push({ type: "status_header", projectPath: "/path", status: "in_progress" });
      for (const t of tasks)
        flatNav.push({ type: "task", task: { key: t.key }, projectPath: "/path" });

      expect(flatNav[0]).toMatchObject({ type: "project_header" });
      expect(flatNav[1]).toMatchObject({ type: "orphan" });
      expect(flatNav[2]).toMatchObject({ type: "status_header", status: "in_progress" });
      expect(flatNav[3]).toMatchObject({ type: "task" });
    });

    it("flatNavIndex provides O(1) lookup matching linear findIndex", () => {
      const flatNav: NavItem[] = [
        { type: "project_header", project: { id: "p1" } },
        { type: "orphan", session: { id: "s1" } },
        { type: "orphan", session: { id: "s2" } },
        { type: "status_header", projectPath: "/proj", status: "in_progress" },
        { type: "task", task: { key: "T-1" }, projectPath: "/proj" },
        { type: "task", task: { key: "T-2" }, projectPath: "/proj" },
        { type: "status_header", projectPath: "/proj", status: "todo" },
        { type: "task", task: { key: "T-3" }, projectPath: "/proj" },
      ];

      const index = buildFlatNavIndex(flatNav);

      expect(index.get("project:p1")).toBe(0);
      expect(index.get("orphan:s1")).toBe(1);
      expect(index.get("orphan:s2")).toBe(2);
      expect(index.get("status:/proj:in_progress")).toBe(3);
      expect(index.get("task:T-1")).toBe(4);
      expect(index.get("task:T-2")).toBe(5);
      expect(index.get("status:/proj:todo")).toBe(6);
      expect(index.get("task:T-3")).toBe(7);
      expect(index.get("nonexistent")).toBeUndefined();
    });

    it("collapsed project only shows project_header", () => {
      type SimpleNavItem = { type: "project_header"; id: string } | { type: "task"; key: string };
      const collapsed = new Set(["project:p1"]);
      const flatNav: SimpleNavItem[] = [];
      flatNav.push({ type: "project_header", id: "p1" });
      if (!collapsed.has("project:p1")) {
        flatNav.push({ type: "task", key: "T-1" });
      }
      expect(flatNav).toHaveLength(1);
      expect(flatNav[0]).toEqual({ type: "project_header", id: "p1" });
    });
  });

  describe("jira section in flat nav", () => {
    interface JiraTaskItem {
      key: string;
      title: string;
      status: string;
      priority: number;
      child_count: number;
    }

    type JiraNavItem =
      | { type: "project_header"; project: { id: string } }
      | { type: "orphan"; session: { id: string } }
      | { type: "status_header"; projectPath: string; status: string }
      | { type: "task"; task: { key: string }; projectPath: string }
      | { type: "jira_header" }
      | { type: "jira_task"; task: { key: string } };

    function makeJiraTask(key: string, status: string, child_count = 0): JiraTaskItem {
      return { key, title: "jira task", status, priority: 0, child_count };
    }

    it("jira section appears after projects when jira tasks exist", () => {
      const flatNav: JiraNavItem[] = [];
      flatNav.push({ type: "project_header", project: { id: "p1" } });
      const jiraTasks = [makeJiraTask("PROJ-1", "todo"), makeJiraTask("PROJ-2", "in_progress")];
      if (jiraTasks.length > 0) {
        flatNav.push({ type: "jira_header" });
        for (const t of jiraTasks) flatNav.push({ type: "jira_task", task: { key: t.key } });
      }
      expect(flatNav).toHaveLength(4);
      expect(flatNav[1]).toMatchObject({ type: "jira_header" });
      expect(flatNav[2]).toMatchObject({ type: "jira_task", task: { key: "PROJ-1" } });
      expect(flatNav[3]).toMatchObject({ type: "jira_task", task: { key: "PROJ-2" } });
    });

    it("jira section is omitted when no jira tasks", () => {
      const flatNav: JiraNavItem[] = [];
      flatNav.push({ type: "project_header", project: { id: "p1" } });
      const jiraTasks: JiraTaskItem[] = [];
      if (jiraTasks.length > 0) {
        flatNav.push({ type: "jira_header" });
      }
      expect(flatNav).toHaveLength(1);
    });

    it("collapsed jira section only shows header", () => {
      const collapsed = new Set(["jira"]);
      const jiraTasks = [makeJiraTask("PROJ-1", "todo")];
      const flatNav: JiraNavItem[] = [];
      flatNav.push({ type: "jira_header" });
      if (!collapsed.has("jira")) {
        for (const t of jiraTasks) flatNav.push({ type: "jira_task", task: { key: t.key } });
      }
      expect(flatNav).toHaveLength(1);
      expect(flatNav[0]).toMatchObject({ type: "jira_header" });
    });

    it("jira tasks remain visible regardless of child creation (Decision 16)", () => {
      // A Jira task that has children should still appear in the Jira section
      const jiraTasks = [makeJiraTask("PROJ-1", "in_progress", 3)];
      const flatNav: JiraNavItem[] = [];
      // Jira section always shows all tasks, even those with children
      flatNav.push({ type: "jira_header" });
      for (const t of jiraTasks) flatNav.push({ type: "jira_task", task: { key: t.key } });
      expect(flatNav).toHaveLength(2);
      expect(jiraTasks[0].child_count).toBe(3);
    });

    it("flatNavIndex includes jira items", () => {
      const flatNav: JiraNavItem[] = [
        { type: "project_header", project: { id: "p1" } },
        { type: "jira_header" },
        { type: "jira_task", task: { key: "PROJ-1" } },
        { type: "jira_task", task: { key: "PROJ-2" } },
      ];
      const map = new Map<string, number>();
      flatNav.forEach((item, i) => {
        if (item.type === "project_header") map.set(`project:${item.project.id}`, i);
        else if (item.type === "jira_header") map.set("jira_header", i);
        else if (item.type === "jira_task") map.set(`jira:${item.task.key}`, i);
      });
      expect(map.get("jira_header")).toBe(1);
      expect(map.get("jira:PROJ-1")).toBe(2);
      expect(map.get("jira:PROJ-2")).toBe(3);
    });
  });
});
