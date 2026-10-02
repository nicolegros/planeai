/**
 * Sidebar model: the single source of the sidebar's sections and row order.
 * Rendering, keyboard navigation and session cycling all derive from it so they never drift apart.
 */
import type { LoopRunSummary, LoopSessionItem, Project, Session, TaskItem } from "./types";
import { shouldHideProject, toLoopId, toTaskWorkspaceId } from "./sidebar-session-order";

export type SidebarGroupBy = "project" | "status";

export const TASK_STATUSES = ["in_progress", "in_review", "todo", "done"] as const;
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

export const SESSIONS_SECTION_KEY = "sessions";

export function projectSectionKey(projectId: string): string {
  return `project:${projectId}`;
}

export function statusSectionKey(status: TaskStatus): string {
  return `status:${status}`;
}

export function projectStatusKey(projectId: string, status: TaskStatus): string {
  return `project-status:${projectId}:${status}`;
}

/** Done groups start collapsed in both modes; every other section starts expanded. */
export function isCollapsedByDefault(key: string): boolean {
  return (
    key === statusSectionKey("done") || (key.startsWith("project-status:") && key.endsWith(":done"))
  );
}

export interface SidebarTaskRow {
  task: TaskItem;
  project: Project;
}

export interface SidebarLoopRow {
  loop: LoopRunSummary;
  project: Project;
}

export interface SidebarSessionRow {
  session: Session;
  project: Project;
}

export interface SidebarStatusGroup {
  key: string;
  status: TaskStatus;
  rows: SidebarTaskRow[];
}

export type SidebarSection =
  | {
      kind: "project";
      key: string;
      project: Project;
      loops: SidebarLoopRow[];
      orphans: SidebarSessionRow[];
      groups: SidebarStatusGroup[];
      /** All of the project's tasks, including ones hidden by hideDoneTasks. */
      taskCount: number;
    }
  | { kind: "sessions"; key: string; loops: SidebarLoopRow[]; orphans: SidebarSessionRow[] }
  | { kind: "status"; key: string; group: SidebarStatusGroup };

export interface SidebarModelInput {
  groupBy: SidebarGroupBy;
  projects: Project[];
  sessions: Session[];
  tasksByProject: Record<string, TaskItem[]>;
  loopsByProject: Record<string, LoopRunSummary[]>;
  loopSessionIds: Set<string>;
  hideDoneTasks: boolean;
  hideEmptyProjects: boolean;
}

export function normalizeTaskStatus(status: string): TaskStatus {
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
    byStatus.set(status, [...(byStatus.get(status) ?? []), row]);
  }
  const groups: SidebarStatusGroup[] = [];
  for (const status of TASK_STATUSES) {
    if (status === "done" && hideDoneTasks) continue;
    const statusRows = byStatus.get(status);
    if (!statusRows?.length) continue;
    statusRows.sort(
      (a, b) =>
        b.task.priority - a.task.priority ||
        (projectOrder.get(a.project.id) ?? 0) - (projectOrder.get(b.project.id) ?? 0) ||
        keyCollator.compare(a.task.key, b.task.key),
    );
    groups.push({ key: keyFor(status), status, rows: statusRows });
  }
  return groups;
}

export function buildSidebarModel(input: SidebarModelInput): SidebarSection[] {
  const {
    groupBy,
    sessions,
    tasksByProject,
    loopsByProject,
    loopSessionIds,
    hideDoneTasks,
    hideEmptyProjects,
  } = input;
  const projects = input.projects.filter((p) => !p.hidden);
  const projectOrder = new Map(projects.map((p, i) => [p.id, i]));
  const allTaskKeys = new Set(
    Object.values(tasksByProject)
      .flat()
      .map((t) => t.key),
  );

  const loopsFor = (project: Project): SidebarLoopRow[] =>
    (loopsByProject[project.id] ?? []).map((loop) => ({ loop, project }));
  const orphansFor = (project: Project): SidebarSessionRow[] =>
    sessions
      .filter(
        (s) =>
          s.project_id === project.id &&
          (!s.task_key || !allTaskKeys.has(s.task_key)) &&
          !loopSessionIds.has(s.id),
      )
      .map((session) => ({ session, project }));
  const tasksFor = (project: Project): SidebarTaskRow[] =>
    (tasksByProject[project.path] ?? []).map((task) => ({ task, project }));

  if (groupBy === "status") {
    const sections: SidebarSection[] = [];
    const loops = projects.flatMap(loopsFor);
    const orphans = projects.flatMap(orphansFor);
    if (loops.length + orphans.length > 0) {
      sections.push({ kind: "sessions", key: SESSIONS_SECTION_KEY, loops, orphans });
    }
    for (const group of groupRows(
      projects.flatMap(tasksFor),
      projectOrder,
      hideDoneTasks,
      statusSectionKey,
    )) {
      sections.push({ kind: "status", key: group.key, group });
    }
    return sections;
  }

  const sections: SidebarSection[] = [];
  for (const project of projects) {
    const loops = loopsFor(project);
    const orphans = orphansFor(project);
    const rows = tasksFor(project);
    const groups = groupRows(rows, projectOrder, hideDoneTasks, (status) =>
      projectStatusKey(project.id, status),
    );
    const visibleTaskCount = groups.reduce((n, g) => n + g.rows.length, 0);
    if (shouldHideProject(orphans.length, visibleTaskCount, hideEmptyProjects, loops.length))
      continue;
    sections.push({
      kind: "project",
      key: projectSectionKey(project.id),
      project,
      loops,
      orphans,
      groups,
      taskCount: rows.length,
    });
  }
  return sections;
}

/**
 * Navigation IDs in sidebar display order, ignoring collapse state.
 * Loop dashboards are "loop:<id>", loop and unlinked sessions are bare session IDs, tasks are task workspace IDs.
 */
export function sidebarNavigationOrder(
  sections: SidebarSection[],
  loopSessions: Record<string, LoopSessionItem[]>,
): string[] {
  const ids: string[] = [];
  const pushLoops = (loops: SidebarLoopRow[]) => {
    for (const { loop } of loops) {
      ids.push(toLoopId(loop.id));
      for (const child of loopSessions[loop.id] ?? []) ids.push(child.session_id);
    }
  };
  const pushTasks = (group: SidebarStatusGroup) => {
    for (const { task, project } of group.rows) ids.push(toTaskWorkspaceId(project.id, task.key));
  };

  for (const section of sections) {
    if (section.kind === "status") {
      pushTasks(section.group);
      continue;
    }
    pushLoops(section.loops);
    for (const { session } of section.orphans) ids.push(session.id);
    if (section.kind === "project") section.groups.forEach(pushTasks);
  }
  return ids;
}
