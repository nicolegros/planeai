import { describe, it, expect } from "vitest";
import { vi } from "vitest";
import { projectContextMenuItems } from "../project-context-menu";
import type { Project, Session, TaskItem } from "../types";

/**
 * Tests that verify context menu item construction logic matches the
 * keyboard action parity requirement (PLA-215).
 *
 * These test the pure logic that determines which items appear in each
 * context menu, extracted from UnifiedSidebar.svelte's template.
 */

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    project_id: "p1",
    name: "my-session",
    tmux_name: null,
    branch: "main",
    status: "active",
    created_at: "",
    worktree_path: null,
    provider: null,
    backend: "direct",
    tab_count: 1,
    base_branch: null,
    task_key: null,
    pr_url: null,
    pr_state: null,
    ...overrides,
  };
}

function makeTask(overrides: Partial<TaskItem> = {}): TaskItem {
  return {
    key: "T-1",
    title: "Test task",
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

// Discriminated union: leaf has onSelect, parent has children
type MenuItem =
  | { label: string; danger?: boolean; onSelect: () => void }
  | { label: string; children: MenuItem[] };

// Shared constant matching UnifiedSidebar
const STATUS_OPTIONS = [
  { value: "todo", label: "Todo" },
  { value: "in_progress", label: "In Progress" },
  { value: "in_review", label: "In Review" },
  { value: "done", label: "Done" },
] as const;

function isParent(item: MenuItem): item is { label: string; children: MenuItem[] } {
  return "children" in item;
}

// Replicate the exited session context menu logic from UnifiedSidebar
function buildExitedSessionMenu(_session: Session): MenuItem[] {
  return [
    { label: "Restart", onSelect: () => {} },
    { label: "Rename", onSelect: () => {} },
    { label: "Archive", onSelect: () => {} },
    { label: "Delete", danger: true, onSelect: () => {} },
  ];
}

// Replicate the active session context menu logic from UnifiedSidebar
function buildActiveSessionMenu(_session: Session): MenuItem[] {
  return [
    { label: "Review", onSelect: () => {} },
    { label: "Rename", onSelect: () => {} },
    { label: "Archive", onSelect: () => {} },
    { label: "Delete", danger: true, onSelect: () => {} },
  ];
}

function makeProject(overrides: Partial<Project> = {}): Project {
  return { id: "p1", name: "Project", path: "/project", hidden: false, ...overrides };
}

// Replicate the task context menu logic from UnifiedSidebar
function buildTaskMenu(task: TaskItem, linkedSession: Session | null): MenuItem[] {
  const statusChildren: MenuItem[] = STATUS_OPTIONS.filter((s) => s.value !== task.status).map(
    (s) => ({ label: s.label, onSelect: () => {} }),
  );

  return [
    ...(linkedSession ? [{ label: "Review diff", onSelect: () => {} } as MenuItem] : []),
    { label: "Edit task", onSelect: () => {} },
    { label: "Change status", children: statusChildren },
    ...(linkedSession
      ? [
          ...(linkedSession.status === "exited"
            ? [{ label: "Restart session", onSelect: () => {} } as MenuItem]
            : []),
          { label: "Rename session", onSelect: () => {} } as MenuItem,
          { label: "Archive session", onSelect: () => {} } as MenuItem,
          { label: "Delete session", danger: true, onSelect: () => {} } as MenuItem,
        ]
      : []),
  ];
}

describe("context menu item construction", () => {
  describe("project menu", () => {
    it("shows Edit project first and invokes its production callback", () => {
      const project = makeProject();
      const onEdit = vi.fn();
      const items = projectContextMenuItems(project, false, {
        onEdit,
        onToggleAutoDispatch: () => {},
        onHide: () => {},
        onArchive: () => {},
        onDelete: () => {},
      });

      expect(items[0].label).toBe("Edit project");
      items[0].onSelect();
      expect(onEdit).toHaveBeenCalledWith(project);
    });

    it("keeps Delete project as a dangerous action", () => {
      const items = projectContextMenuItems(makeProject(), false, {
        onEdit: () => {},
        onToggleAutoDispatch: () => {},
        onHide: () => {},
        onArchive: () => {},
        onDelete: () => {},
      });
      const deleteItem = items.find((item) => item.label === "Delete project");
      expect(deleteItem?.danger).toBe(true);
    });
  });

  describe("exited session menu", () => {
    it("includes Restart, Rename, Archive, Delete", () => {
      const session = makeSession({ status: "exited" });
      const items = buildExitedSessionMenu(session);
      const labels = items.map((i) => i.label);
      expect(labels).toEqual(["Restart", "Rename", "Archive", "Delete"]);
    });

    it("marks Delete as danger", () => {
      const session = makeSession({ status: "exited" });
      const items = buildExitedSessionMenu(session);
      const del = items.find((i) => i.label === "Delete");
      expect(del && !isParent(del) && del.danger).toBe(true);
    });

    it("does not mark non-destructive items as danger", () => {
      const session = makeSession({ status: "exited" });
      const items = buildExitedSessionMenu(session);
      const dangerItems = items.filter((i) => !isParent(i) && i.danger);
      expect(dangerItems).toHaveLength(1);
    });
  });

  describe("active session menu", () => {
    it("includes Review, Rename, Archive, Delete", () => {
      const session = makeSession({ status: "active" });
      const items = buildActiveSessionMenu(session);
      const labels = items.map((i) => i.label);
      expect(labels).toEqual(["Review", "Rename", "Archive", "Delete"]);
    });

    it("marks Delete as danger", () => {
      const session = makeSession({ status: "active" });
      const items = buildActiveSessionMenu(session);
      const del = items.find((i) => i.label === "Delete");
      expect(del && !isParent(del) && del.danger).toBe(true);
    });
  });

  describe("task menu", () => {
    it("shows 'Edit task' as first item when no linked session", () => {
      const task = makeTask({ status: "todo" });
      const items = buildTaskMenu(task, null);
      expect(items[0].label).toBe("Edit task");
    });

    it("shows 'Review diff' as first item when linked session exists", () => {
      const task = makeTask({ status: "in_progress" });
      const linked = makeSession({ task_key: "T-1" });
      const items = buildTaskMenu(task, linked);
      expect(items[0].label).toBe("Review diff");
    });

    it("shows 'Review diff' only when linked session exists", () => {
      const task = makeTask({ status: "in_progress" });
      const linked = makeSession({ task_key: "T-1" });
      const items = buildTaskMenu(task, linked);
      expect(items.find((i) => i.label === "Review diff")).toBeDefined();
    });

    it("does not show 'Review diff' when no linked session", () => {
      const task = makeTask({ status: "todo" });
      const items = buildTaskMenu(task, null);
      expect(items.find((i) => i.label === "Review diff")).toBeUndefined();
    });

    it("always shows 'Edit task'", () => {
      const task = makeTask({ status: "done" });
      const items = buildTaskMenu(task, null);
      expect(items.find((i) => i.label === "Edit task")).toBeDefined();
    });

    it("has 'Change status' with children submenu", () => {
      const task = makeTask({ status: "todo" });
      const items = buildTaskMenu(task, null);
      const statusItem = items.find((i) => i.label === "Change status");
      expect(statusItem).toBeDefined();
      expect(isParent(statusItem!)).toBe(true);
      if (isParent(statusItem!)) {
        expect(statusItem!.children.length).toBeGreaterThan(0);
      }
    });

    it("excludes current status from submenu children", () => {
      const task = makeTask({ status: "in_progress" });
      const items = buildTaskMenu(task, null);
      const statusItem = items.find((i) => isParent(i))!;
      if (isParent(statusItem)) {
        const childLabels = statusItem.children.map((c) => c.label);
        expect(childLabels).not.toContain("In Progress");
        expect(childLabels).toContain("Todo");
        expect(childLabels).toContain("In Review");
        expect(childLabels).toContain("Done");
      }
    });

    it("excludes 'Todo' from submenu when task is already todo", () => {
      const task = makeTask({ status: "todo" });
      const items = buildTaskMenu(task, null);
      const statusItem = items.find((i) => isParent(i))!;
      if (isParent(statusItem)) {
        const childLabels = statusItem.children.map((c) => c.label);
        expect(childLabels).not.toContain("Todo");
        expect(childLabels).toContain("In Progress");
        expect(childLabels).toContain("In Review");
        expect(childLabels).toContain("Done");
      }
    });

    it("excludes 'Done' from submenu when task is already done", () => {
      const task = makeTask({ status: "done" });
      const items = buildTaskMenu(task, null);
      const statusItem = items.find((i) => isParent(i))!;
      if (isParent(statusItem)) {
        const childLabels = statusItem.children.map((c) => c.label);
        expect(childLabels).not.toContain("Done");
        expect(childLabels).toContain("Todo");
        expect(childLabels).toContain("In Progress");
        expect(childLabels).toContain("In Review");
      }
    });

    it("shows all menu items in correct order with linked session", () => {
      const task = makeTask({ status: "in_progress" });
      const linked = makeSession({ task_key: "T-1" });
      const items = buildTaskMenu(task, linked);
      const labels = items.map((i) => i.label);
      expect(labels).toEqual([
        "Review diff",
        "Edit task",
        "Change status",
        "Rename session",
        "Archive session",
        "Delete session",
      ]);
    });

    it("shows all menu items in correct order with exited linked session", () => {
      const task = makeTask({ status: "in_progress" });
      const linked = makeSession({ task_key: "T-1", status: "exited" });
      const items = buildTaskMenu(task, linked);
      const labels = items.map((i) => i.label);
      expect(labels).toEqual([
        "Review diff",
        "Edit task",
        "Change status",
        "Restart session",
        "Rename session",
        "Archive session",
        "Delete session",
      ]);
    });

    it("marks 'Delete session' as danger in task menu", () => {
      const task = makeTask({ status: "in_progress" });
      const linked = makeSession({ task_key: "T-1" });
      const items = buildTaskMenu(task, linked);
      const del = items.find((i) => i.label === "Delete session");
      expect(del && !isParent(del) && del.danger).toBe(true);
    });

    it("does not show session actions when no linked session", () => {
      const task = makeTask({ status: "todo" });
      const items = buildTaskMenu(task, null);
      const labels = items.map((i) => i.label);
      expect(labels).not.toContain("Delete session");
      expect(labels).not.toContain("Archive session");
      expect(labels).not.toContain("Rename session");
      expect(labels).not.toContain("Restart session");
    });

    it("shows all menu items in correct order without linked session", () => {
      const task = makeTask({ status: "todo" });
      const items = buildTaskMenu(task, null);
      const labels = items.map((i) => i.label);
      expect(labels).toEqual(["Edit task", "Change status"]);
    });
  });
});
