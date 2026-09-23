import { describe, expect, it } from "vitest";
import { computeCommandScore } from "bits-ui";
import {
  computeCommandMenuSessions,
  computeCommandMenuTaskRows,
  matchableValue,
  taskRowValue,
} from "../command-menu-items";
import type { Project, Session, TaskItem } from "../types";

function makeProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    name: "Project One",
    path: "/repo/one",
    hidden: false,
    ...overrides,
  } as Project;
}

function makeTask(overrides: Partial<TaskItem> = {}): TaskItem {
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

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "session-1",
    project_id: "project-1",
    name: "Session one",
    branch: "feature",
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
  } as Session;
}

describe("computeCommandMenuTaskRows", () => {
  it("returns tasks from every project, carrying the repo path and project id", () => {
    const projects = [
      makeProject(),
      makeProject({ id: "project-2", name: "Project Two", path: "/repo/two" }),
    ];
    const rows = computeCommandMenuTaskRows(
      projects,
      {
        "/repo/one": [makeTask({ key: "PLA-1" })],
        "/repo/two": [makeTask({ key: "TWO-1" })],
      },
      null,
      false,
    );

    expect(rows.map((row) => [row.task.key, row.repoPath, row.projectId])).toEqual([
      ["PLA-1", "/repo/one", "project-1"],
      ["TWO-1", "/repo/two", "project-2"],
    ]);
  });

  it("works with no active project, so tasks are searchable without an active session", () => {
    const rows = computeCommandMenuTaskRows(
      [makeProject()],
      { "/repo/one": [makeTask()] },
      null,
      false,
    );

    expect(rows).toHaveLength(1);
  });

  it("orders the active project's tasks before other projects", () => {
    const projects = [
      makeProject(),
      makeProject({ id: "project-2", name: "Project Two", path: "/repo/two" }),
    ];
    const rows = computeCommandMenuTaskRows(
      projects,
      {
        "/repo/one": [makeTask({ key: "PLA-1" })],
        "/repo/two": [makeTask({ key: "TWO-1" })],
      },
      "/repo/two",
      false,
    );

    expect(rows.map((row) => row.task.key)).toEqual(["TWO-1", "PLA-1"]);
  });

  it("orders tasks by status then descending priority within a project", () => {
    const rows = computeCommandMenuTaskRows(
      [makeProject()],
      {
        "/repo/one": [
          makeTask({ key: "PLA-done", status: "done" }),
          makeTask({ key: "PLA-todo-low", status: "todo", priority: 1 }),
          makeTask({ key: "PLA-review", status: "in_review" }),
          makeTask({ key: "PLA-todo-high", status: "todo", priority: 5 }),
          makeTask({ key: "PLA-progress", status: "in_progress" }),
        ],
      },
      null,
      false,
    );

    expect(rows.map((row) => row.task.key)).toEqual([
      "PLA-progress",
      "PLA-review",
      "PLA-todo-high",
      "PLA-todo-low",
      "PLA-done",
    ]);
  });

  it("excludes done tasks when hideDone is set", () => {
    const rows = computeCommandMenuTaskRows(
      [makeProject()],
      {
        "/repo/one": [
          makeTask({ key: "PLA-1", status: "todo" }),
          makeTask({ key: "PLA-2", status: "done" }),
        ],
      },
      null,
      true,
    );

    expect(rows.map((row) => row.task.key)).toEqual(["PLA-1"]);
  });

  it("keeps tasks with a status outside the known order rather than dropping them", () => {
    const rows = computeCommandMenuTaskRows(
      [makeProject()],
      { "/repo/one": [makeTask({ key: "PLA-9", status: "blocked" })] },
      null,
      false,
    );

    expect(rows.map((row) => row.task.key)).toEqual(["PLA-9"]);
  });

  it("ignores task entries for repo paths with no matching project", () => {
    const rows = computeCommandMenuTaskRows(
      [makeProject()],
      {
        "/repo/one": [makeTask({ key: "PLA-1" })],
        "/repo/gone": [makeTask({ key: "GONE-1" })],
      },
      null,
      false,
    );

    expect(rows.map((row) => row.task.key)).toEqual(["PLA-1"]);
  });
});

describe("taskRowValue", () => {
  it("disambiguates identical task keys from different projects", () => {
    const a = taskRowValue({ task: makeTask(), repoPath: "/repo/one", projectId: "project-1" });
    const b = taskRowValue({ task: makeTask(), repoPath: "/repo/two", projectId: "project-2" });

    expect(a).not.toEqual(b);
  });

  it("includes the key and the title so both are searchable", () => {
    const value = taskRowValue({
      task: makeTask({ key: "PLA-42", title: "Fix the launcher" }),
      repoPath: "/repo/one",
      projectId: "project-1",
    });

    expect(matchableValue(value)).toBe("PLA-42 Fix the launcher");
  });

  it("keeps the repo path out of the matchable text", () => {
    const value = taskRowValue({
      task: makeTask({ key: "PLA-42", title: "Fix the launcher" }),
      repoPath: "/repo/secret-directory",
      projectId: "project-1",
    });

    expect(value).toContain("/repo/secret-directory");
    expect(matchableValue(value)).not.toContain("secret-directory");
  });
});

describe("matchableValue", () => {
  it("returns values without a separator unchanged", () => {
    expect(matchableValue("create new session")).toBe("create new session");
  });

  it("does not let a query match a task through its repo path", () => {
    const value = taskRowValue({
      task: makeTask({ key: "PLA-1", title: "Task one" }),
      repoPath: "/Users/someone/worktrees/planeai",
      projectId: "project-1",
    });

    expect(computeCommandScore(matchableValue(value), "worktrees")).toBe(0);
    expect(computeCommandScore(matchableValue(value), "Task one")).toBeGreaterThan(0);
  });
});

describe("computeCommandMenuSessions", () => {
  const rows = computeCommandMenuTaskRows(
    [makeProject()],
    { "/repo/one": [makeTask({ key: "PLA-1" })] },
    null,
    false,
  );

  it("excludes sessions a visible task can route to", () => {
    const sessions = [makeSession({ id: "linked", task_key: "PLA-1" })];

    expect(computeCommandMenuSessions(sessions, rows, () => false)).toEqual([]);
  });

  it("includes sessions with no task key", () => {
    const sessions = [makeSession({ id: "orphan", task_key: null })];

    expect(computeCommandMenuSessions(sessions, rows, () => false).map((s) => s.id)).toEqual([
      "orphan",
    ]);
  });

  it("includes sessions whose task key is not among the visible rows", () => {
    const sessions = [makeSession({ id: "stale", task_key: "PLA-404" })];

    expect(computeCommandMenuSessions(sessions, rows, () => false).map((s) => s.id)).toEqual([
      "stale",
    ]);
  });

  it("includes sessions whose task belongs to a different project", () => {
    const sessions = [makeSession({ id: "other", project_id: "project-2", task_key: "PLA-1" })];

    expect(computeCommandMenuSessions(sessions, rows, () => false).map((s) => s.id)).toEqual([
      "other",
    ]);
  });

  it("includes loop sessions even when a task could route to them", () => {
    const sessions = [makeSession({ id: "loop-session", task_key: "PLA-1" })];

    expect(
      computeCommandMenuSessions(sessions, rows, (id) => id === "loop-session").map((s) => s.id),
    ).toEqual(["loop-session"]);
  });

  it("includes sessions linked to a done task that hideDone removed from the rows", () => {
    const hiddenDoneRows = computeCommandMenuTaskRows(
      [makeProject()],
      { "/repo/one": [makeTask({ key: "PLA-1", status: "done" })] },
      null,
      true,
    );
    const sessions = [makeSession({ id: "done-linked", task_key: "PLA-1" })];

    expect(
      computeCommandMenuSessions(sessions, hiddenDoneRows, () => false).map((s) => s.id),
    ).toEqual(["done-linked"]);
  });
});
