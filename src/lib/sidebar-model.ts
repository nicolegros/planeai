/**
 * Sidebar model: the single source of the sidebar's sections and row order.
 * Rendering, keyboard navigation and session cycling all derive from it so they never drift apart.
 */
import {
  sessionTaskProjectId,
  type LoopRunSummary,
  type LoopSessionItem,
  type Project,
  type Session,
  type TaskItem,
} from "./types";
import { shouldHideProject, toLoopId, toTaskWorkspaceId } from "./sidebar-session-order";

export type SidebarGroupBy = "project" | "status";

export function resolveSidebarGroupBy(settings: {
  sidebar_group_by?: SidebarGroupBy | null;
}): SidebarGroupBy {
  return settings.sidebar_group_by ?? "project";
}

const TASK_STATUSES = ["in_progress", "in_review", "todo", "done"] as const;
export type TaskStatus = (typeof TASK_STATUSES)[number];

export const TASK_STATUS_LABELS: Record<TaskStatus, string> = {
  in_progress: "Running",
  in_review: "Needs review",
  todo: "To do",
  done: "Done",
};

export const TASK_STATUS_DOT_CLASSES: Record<TaskStatus, string> = {
  in_progress: "bg-status-running",
  in_review: "bg-status-review",
  todo: "bg-t3",
  done: "bg-status-running",
};

export interface SidebarTaskRow {
  task: TaskItem;
  project: Project;
  linkedSession: Session | undefined;
}

export interface SidebarLoopRow {
  loop: LoopRunSummary;
  project: Project;
  children: { item: LoopSessionItem; session: Session }[];
}

export interface SidebarSessionRow {
  session: Session;
  project: Project;
}

export interface SidebarStatusGroup {
  key: string;
  status: TaskStatus;
  rows: SidebarTaskRow[];
  defaultCollapsed: boolean;
}

export interface SidebarProjectSection {
  kind: "project";
  key: string;
  project: Project;
  loops: SidebarLoopRow[];
  orphans: SidebarSessionRow[];
  groups: SidebarStatusGroup[];
  /** All of the project's tasks, including ones hidden by hideDoneTasks. */
  taskCount: number;
  defaultCollapsed: boolean;
}

export type SidebarSection =
  | SidebarProjectSection
  | {
      kind: "sessions";
      key: string;
      loops: SidebarLoopRow[];
      orphans: SidebarSessionRow[];
      defaultCollapsed: boolean;
    }
  | { kind: "status"; key: string; group: SidebarStatusGroup; defaultCollapsed: boolean };

export interface SidebarModelInput {
  groupBy: SidebarGroupBy;
  projects: Project[];
  sessions: Session[];
  tasksByProject: Record<string, TaskItem[]>;
  loopsByProject: Record<string, LoopRunSummary[]>;
  loopSessions: Record<string, LoopSessionItem[]>;
  hideDoneTasks: boolean;
  hideEmptyProjects: boolean;
}

function normalizeTaskStatus(status: string): TaskStatus {
  return (TASK_STATUSES as readonly string[]).includes(status) ? (status as TaskStatus) : "todo";
}

const keyCollator = new Intl.Collator(undefined, { numeric: true });

function groupRows(
  rows: SidebarTaskRow[],
  projectOrder: Map<string, number>,
  hideDoneTasks: boolean,
  keyFor: (status: TaskStatus) => string,
): SidebarStatusGroup[] {
  const byStatus = new Map<TaskStatus, SidebarTaskRow[]>();
  for (const row of rows) {
    const status = normalizeTaskStatus(row.task.status);
    let list = byStatus.get(status);
    if (!list) byStatus.set(status, (list = []));
    list.push(row);
  }
  const groups: SidebarStatusGroup[] = [];
  for (const status of TASK_STATUSES) {
    if (status === "done" && hideDoneTasks) continue;
    const statusRows = byStatus.get(status);
    if (!statusRows) continue;
    statusRows.sort(
      (a, b) =>
        b.task.priority - a.task.priority ||
        (projectOrder.get(a.project.id) ?? 0) - (projectOrder.get(b.project.id) ?? 0) ||
        keyCollator.compare(a.task.key, b.task.key),
    );
    groups.push({
      key: keyFor(status),
      status,
      rows: statusRows,
      defaultCollapsed: status === "done",
    });
  }
  return groups;
}

export function buildSidebarModel(input: SidebarModelInput): SidebarSection[] {
  const {
    groupBy,
    sessions,
    tasksByProject,
    loopsByProject,
    loopSessions,
    hideDoneTasks,
    hideEmptyProjects,
  } = input;
  const projects = input.projects.filter((p) => !p.hidden);
  const projectOrder = new Map(projects.map((p, i) => [p.id, i]));

  // Tasks are identified by project and key: keys may repeat across projects.
  const taskId = (projectId: string, key: string) => `${projectId}:${key}`;
  const existingTasks = new Set(
    input.projects.flatMap((p) => (tasksByProject[p.path] ?? []).map((t) => taskId(p.id, t.key))),
  );
  const sessionsById = new Map(sessions.map((s) => [s.id, s]));
  const sessionsByTask = new Map<string, Session>();
  for (const s of sessions) {
    if (!s.task_key) continue;
    const id = taskId(sessionTaskProjectId(s), s.task_key);
    if (!sessionsByTask.has(id)) sessionsByTask.set(id, s);
  }
  const loopSessionIds = new Set(
    Object.values(loopSessions).flatMap((items) => items.map((i) => i.session_id)),
  );

  const loopsFor = (project: Project): SidebarLoopRow[] =>
    (loopsByProject[project.id] ?? []).map((loop) => ({
      loop,
      project,
      children: (loopSessions[loop.id] ?? []).flatMap((item) => {
        const session = sessionsById.get(item.session_id);
        return session ? [{ item, session }] : [];
      }),
    }));
  const orphansFor = (project: Project): SidebarSessionRow[] =>
    sessions
      .filter(
        (s) =>
          s.project_id === project.id &&
          !loopSessionIds.has(s.id) &&
          (!s.task_key || !existingTasks.has(taskId(sessionTaskProjectId(s), s.task_key))),
      )
      .map((session) => ({ session, project }));
  const tasksFor = (project: Project): SidebarTaskRow[] =>
    (tasksByProject[project.path] ?? []).map((task) => ({
      task,
      project,
      linkedSession: sessionsByTask.get(taskId(project.id, task.key)),
    }));

  if (groupBy === "status") {
    const sections: SidebarSection[] = [];
    const loops = projects.flatMap(loopsFor);
    const orphans = projects.flatMap(orphansFor);
    if (loops.length + orphans.length > 0) {
      sections.push({ kind: "sessions", key: "sessions", loops, orphans, defaultCollapsed: false });
    }
    const groups = groupRows(
      projects.flatMap(tasksFor),
      projectOrder,
      hideDoneTasks,
      (s) => `status:${s}`,
    );
    for (const group of groups) {
      sections.push({
        kind: "status",
        key: group.key,
        group,
        defaultCollapsed: group.defaultCollapsed,
      });
    }
    return sections;
  }

  const sections: SidebarSection[] = [];
  for (const project of projects) {
    const loops = loopsFor(project);
    const orphans = orphansFor(project);
    const rows = tasksFor(project);
    const groups = groupRows(
      rows,
      projectOrder,
      hideDoneTasks,
      (status) => `project-status:${project.id}:${status}`,
    );
    const visibleTaskCount = groups.reduce((n, g) => n + g.rows.length, 0);
    if (shouldHideProject(orphans.length, visibleTaskCount, hideEmptyProjects, loops.length))
      continue;
    sections.push({
      kind: "project",
      key: `project:${project.id}`,
      project,
      loops,
      orphans,
      groups,
      taskCount: rows.length,
      // Empty projects start collapsed.
      defaultCollapsed: orphans.length === 0 && rows.length === 0,
    });
  }
  return sections;
}

export type SidebarNavItem =
  | { type: "project_header"; section: SidebarProjectSection }
  | { type: "group_header"; key: string; parentKey: string | null; defaultCollapsed: boolean }
  | { type: "loop"; loop: LoopRunSummary }
  | { type: "loop_session"; session: Session; loopId: string; item: LoopSessionItem }
  | { type: "orphan"; session: Session }
  | { type: "task"; row: SidebarTaskRow };

/** Resolves a section's collapse state; `fallback` is its default when the user never toggled it. */
export type IsCollapsed = (key: string, fallback: boolean) => boolean;

/** Lookup keys of nav items, shared by sidebarNavItemKey and every index lookup. */
export const navKey = {
  project: (sectionKey: string) => sectionKey,
  group: (sectionKey: string) => `group:${sectionKey}`,
  loop: (loopId: string) => toLoopId(loopId),
  loopSession: (sessionId: string) => `loop_session:${sessionId}`,
  orphan: (sessionId: string) => `orphan:${sessionId}`,
  task: (projectId: string, taskKey: string) => toTaskWorkspaceId(projectId, taskKey),
};

/** Sidebar rows in display order, skipping the content of collapsed sections. */
export function sidebarNavItems(
  sections: SidebarSection[],
  isCollapsed: IsCollapsed = () => false,
): SidebarNavItem[] {
  const items: SidebarNavItem[] = [];
  const pushSessions = (loops: SidebarLoopRow[], orphans: SidebarSessionRow[]) => {
    for (const { loop, children } of loops) {
      items.push({ type: "loop", loop });
      // Loops start expanded and use their loop ID as collapse key.
      if (isCollapsed(toLoopId(loop.id), false)) continue;
      for (const { item, session } of children)
        items.push({ type: "loop_session", session, loopId: loop.id, item });
    }
    for (const { session } of orphans) items.push({ type: "orphan", session });
  };
  const pushGroup = (group: SidebarStatusGroup, parentKey: string | null) => {
    items.push({
      type: "group_header",
      key: group.key,
      parentKey,
      defaultCollapsed: group.defaultCollapsed,
    });
    if (isCollapsed(group.key, group.defaultCollapsed)) return;
    for (const row of group.rows) items.push({ type: "task", row });
  };

  for (const section of sections) {
    if (section.kind === "project") {
      items.push({ type: "project_header", section });
      if (isCollapsed(section.key, section.defaultCollapsed)) continue;
      pushSessions(section.loops, section.orphans);
      for (const group of section.groups) pushGroup(group, section.key);
    } else if (section.kind === "sessions") {
      items.push({
        type: "group_header",
        key: section.key,
        parentKey: null,
        defaultCollapsed: section.defaultCollapsed,
      });
      if (isCollapsed(section.key, section.defaultCollapsed)) continue;
      pushSessions(section.loops, section.orphans);
    } else {
      pushGroup(section.group, null);
    }
  }
  return items;
}

/** Unique lookup key of a nav item, used to find its index in the flat list. */
export function sidebarNavItemKey(item: SidebarNavItem): string {
  switch (item.type) {
    case "project_header":
      return navKey.project(item.section.key);
    case "group_header":
      return navKey.group(item.key);
    case "loop":
      return navKey.loop(item.loop.id);
    case "loop_session":
      return navKey.loopSession(item.session.id);
    case "orphan":
      return navKey.orphan(item.session.id);
    case "task":
      return navKey.task(item.row.project.id, item.row.task.key);
  }
}

/**
 * Navigation IDs in sidebar display order, ignoring collapse state.
 * Loop dashboards are "loop:<id>", loop and unlinked sessions are bare session IDs, tasks are task workspace IDs.
 */
export function sidebarNavigationOrder(sections: SidebarSection[]): string[] {
  return sidebarNavItems(sections).flatMap((item) => {
    if (item.type === "loop") return [toLoopId(item.loop.id)];
    if (item.type === "loop_session" || item.type === "orphan") return [item.session.id];
    if (item.type === "task") return [toTaskWorkspaceId(item.row.project.id, item.row.task.key)];
    return [];
  });
}
