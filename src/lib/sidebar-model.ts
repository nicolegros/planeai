/**
 * Sidebar model: the single source of the sidebar's sections and row order.
 * Rendering, keyboard navigation and session cycling all derive from it so they never drift apart.
 */
import { sessionTaskProjectId, type Project, type Session, type TaskItem } from "./types";
import { shouldHideProject, toTaskWorkspaceId } from "./sidebar-session-order";

export type SidebarGroupBy = "project" | "status";
export type SidebarLayout = "classic" | "cards";

export function resolveSidebarLayout(settings: {
  sidebar_layout?: SidebarLayout | null;
}): SidebarLayout {
  return settings.sidebar_layout ?? "cards";
}

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
  kind: "task";
  task: TaskItem;
  project: Project;
  linkedSession: Session | undefined;
}

/** A session linked to no existing task. */
export interface SidebarSessionRow {
  kind: "session";
  session: Session;
  project: Project;
}

export type SidebarRow = SidebarTaskRow | SidebarSessionRow;

/** Cards keeps `in_review` tasks on top with unlinked sessions; every other status gets a shelf. */
export type ShelfStatus = Exclude<TaskStatus, "in_review">;

export interface Shelf {
  variant: "card" | "slim";
  defaultCollapsed: boolean;
  /** Rows shown per "Show more" step; null shows every row. */
  pageSize: number | null;
}

/** Cards shelves in display order. Their labels are TASK_STATUS_LABELS. */
export const SHELVES: Record<ShelfStatus, Shelf> = {
  in_progress: { variant: "card", defaultCollapsed: false, pageSize: null },
  todo: { variant: "slim", defaultCollapsed: false, pageSize: null },
  done: { variant: "slim", defaultCollapsed: true, pageSize: 25 },
};

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
  orphans: SidebarSessionRow[];
  groups: SidebarStatusGroup[];
  /** All of the project's tasks, including ones hidden by hideDoneTasks. */
  taskCount: number;
  defaultCollapsed: boolean;
}

/** A project's top rows in Cards grouped by project. */
export interface SidebarFolderSection {
  kind: "folder";
  key: string;
  project: Project;
  rows: SidebarRow[];
  defaultCollapsed: boolean;
}

export interface SidebarShelfSection {
  kind: "shelf";
  key: string;
  status: ShelfStatus;
  rows: SidebarTaskRow[];
  defaultCollapsed: boolean;
}

export type SidebarSection =
  // Classic
  | SidebarProjectSection
  | {
      kind: "sessions";
      key: string;
      orphans: SidebarSessionRow[];
      defaultCollapsed: boolean;
    }
  | { kind: "status"; key: string; group: SidebarStatusGroup; defaultCollapsed: boolean }
  // Cards
  | { kind: "attention"; key: string; rows: SidebarRow[] }
  | SidebarFolderSection
  | SidebarShelfSection;

export interface SidebarModelInput {
  layout: SidebarLayout;
  groupBy: SidebarGroupBy;
  projects: Project[];
  sessions: Session[];
  tasksByProject: Record<string, TaskItem[]>;
  hideDoneTasks: boolean;
  hideEmptyProjects: boolean;
}

function normalizeTaskStatus(status: string): TaskStatus {
  return (TASK_STATUSES as readonly string[]).includes(status) ? (status as TaskStatus) : "todo";
}

const keyCollator = new Intl.Collator(undefined, { numeric: true });

/** Rows of one status in sidebar order. */
function rowsWithStatus(
  rows: SidebarTaskRow[],
  status: TaskStatus,
  projectOrder: Map<string, number>,
): SidebarTaskRow[] {
  return rows
    .filter((row) => normalizeTaskStatus(row.task.status) === status)
    .sort(
      (a, b) =>
        b.task.priority - a.task.priority ||
        (projectOrder.get(a.project.id) ?? 0) - (projectOrder.get(b.project.id) ?? 0) ||
        keyCollator.compare(a.task.key, b.task.key),
    );
}

function groupRows(
  rows: SidebarTaskRow[],
  projectOrder: Map<string, number>,
  hideDoneTasks: boolean,
  keyFor: (status: TaskStatus) => string,
): SidebarStatusGroup[] {
  const groups: SidebarStatusGroup[] = [];
  for (const status of TASK_STATUSES) {
    if (status === "done" && hideDoneTasks) continue;
    const statusRows = rowsWithStatus(rows, status, projectOrder);
    if (statusRows.length === 0) continue;
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
  const { layout, groupBy, sessions, tasksByProject, hideDoneTasks, hideEmptyProjects } = input;
  const projects = input.projects.filter((p) => !p.hidden);
  const projectOrder = new Map(projects.map((p, i) => [p.id, i]));

  // Tasks are identified by project and key: keys may repeat across projects.
  const taskId = (projectId: string, key: string) => `${projectId}:${key}`;
  const existingTasks = new Set(
    input.projects.flatMap((p) => (tasksByProject[p.path] ?? []).map((t) => taskId(p.id, t.key))),
  );
  const sessionsByTask = new Map<string, Session>();
  for (const s of sessions) {
    if (!s.task_key) continue;
    const id = taskId(sessionTaskProjectId(s), s.task_key);
    if (!sessionsByTask.has(id)) sessionsByTask.set(id, s);
  }

  const orphansFor = (project: Project): SidebarSessionRow[] =>
    sessions
      .filter(
        (s) =>
          s.project_id === project.id &&
          (!s.task_key || !existingTasks.has(taskId(sessionTaskProjectId(s), s.task_key))),
      )
      .map((session) => ({ kind: "session", session, project }));
  const tasksFor = (project: Project): SidebarTaskRow[] =>
    (tasksByProject[project.path] ?? []).map((task) => ({
      kind: "task",
      task,
      project,
      linkedSession: sessionsByTask.get(taskId(project.id, task.key)),
    }));

  if (layout === "cards") {
    const shown = projects
      .map((project) => ({
        project,
        orphans: orphansFor(project),
        tasks: tasksFor(project).filter(
          (row) => !(hideDoneTasks && normalizeTaskStatus(row.task.status) === "done"),
        ),
      }))
      .filter((p) => !shouldHideProject(p.orphans.length, p.tasks.length, hideEmptyProjects));
    // Unlinked sessions keep their order and come before the tasks waiting for review.
    const topRows = (orphans: SidebarSessionRow[], tasks: SidebarTaskRow[]): SidebarRow[] => [
      ...orphans,
      ...rowsWithStatus(tasks, "in_review", projectOrder),
    ];

    const sections: SidebarSection[] = [];
    if (groupBy === "project") {
      for (const { project, orphans, tasks } of shown) {
        const rows = topRows(orphans, tasks);
        sections.push({
          kind: "folder",
          key: `folder:${project.id}`,
          project,
          rows,
          defaultCollapsed: rows.length === 0,
        });
      }
    } else {
      const rows = topRows(
        shown.flatMap((p) => p.orphans),
        shown.flatMap((p) => p.tasks),
      );
      if (rows.length > 0) sections.push({ kind: "attention", key: "attention", rows });
    }
    const allTasks = shown.flatMap((p) => p.tasks);
    for (const status of Object.keys(SHELVES) as ShelfStatus[]) {
      const rows = rowsWithStatus(allTasks, status, projectOrder);
      if (rows.length === 0) continue;
      sections.push({
        kind: "shelf",
        key: `shelf:${status}`,
        status,
        rows,
        defaultCollapsed: SHELVES[status].defaultCollapsed,
      });
    }
    return sections;
  }

  if (groupBy === "status") {
    const sections: SidebarSection[] = [];
    const orphans = projects.flatMap(orphansFor);
    if (orphans.length > 0) {
      sections.push({ kind: "sessions", key: "sessions", orphans, defaultCollapsed: false });
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
    const orphans = orphansFor(project);
    const rows = tasksFor(project);
    const groups = groupRows(
      rows,
      projectOrder,
      hideDoneTasks,
      (status) => `project-status:${project.id}:${status}`,
    );
    const visibleTaskCount = groups.reduce((n, g) => n + g.rows.length, 0);
    if (shouldHideProject(orphans.length, visibleTaskCount, hideEmptyProjects)) continue;
    sections.push({
      kind: "project",
      key: `project:${project.id}`,
      project,
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
  | { type: "project_header"; section: SidebarProjectSection | SidebarFolderSection }
  | { type: "group_header"; key: string; parentKey: string | null; defaultCollapsed: boolean }
  | { type: "orphan"; session: Session }
  | { type: "task"; row: SidebarTaskRow }
  | { type: "show_more"; shelfKey: string };

/** Resolves a section's collapse state; `fallback` is its default when the user never toggled it. */
export type IsCollapsed = (key: string, fallback: boolean) => boolean;

/** How many pages of a shelf are open. */
export type ShownPages = (shelfKey: string) => number;

/** The rows a paged shelf shows, and how many the next "Show more" adds. */
export function shelfPage(
  shelf: SidebarShelfSection,
  pages: number,
): { rows: SidebarTaskRow[]; more: number } {
  const { pageSize } = SHELVES[shelf.status];
  if (pageSize === null) return { rows: shelf.rows, more: 0 };
  const rows = shelf.rows.slice(0, pageSize * pages);
  return { rows, more: Math.min(pageSize, shelf.rows.length - rows.length) };
}

/** Lookup keys of nav items, shared by sidebarNavItemKey and every index lookup. */
export const navKey = {
  project: (sectionKey: string) => sectionKey,
  group: (sectionKey: string) => `group:${sectionKey}`,
  orphan: (sessionId: string) => `orphan:${sessionId}`,
  task: (projectId: string, taskKey: string) => toTaskWorkspaceId(projectId, taskKey),
  more: (shelfKey: string) => `more:${shelfKey}`,
};

/** Nav key of a row, task or session. */
export function sidebarRowNavKey(row: SidebarRow): string {
  return row.kind === "task"
    ? navKey.task(row.project.id, row.task.key)
    : navKey.orphan(row.session.id);
}

/**
 * Sidebar rows in display order, skipping the content of collapsed sections
 * and the shelf rows past their open pages. By default everything is open.
 */
export function sidebarNavItems(
  sections: SidebarSection[],
  isCollapsed: IsCollapsed = () => false,
  shownPages: ShownPages = () => Infinity,
): SidebarNavItem[] {
  const items: SidebarNavItem[] = [];
  const pushRows = (rows: SidebarRow[]) => {
    for (const row of rows) {
      items.push(
        row.kind === "task" ? { type: "task", row } : { type: "orphan", session: row.session },
      );
    }
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
    switch (section.kind) {
      case "project":
        items.push({ type: "project_header", section });
        if (isCollapsed(section.key, section.defaultCollapsed)) break;
        pushRows(section.orphans);
        for (const group of section.groups) pushGroup(group, section.key);
        break;
      case "sessions":
        items.push({
          type: "group_header",
          key: section.key,
          parentKey: null,
          defaultCollapsed: section.defaultCollapsed,
        });
        if (isCollapsed(section.key, section.defaultCollapsed)) break;
        pushRows(section.orphans);
        break;
      case "status":
        pushGroup(section.group, null);
        break;
      case "attention":
        pushRows(section.rows);
        break;
      case "folder":
        items.push({ type: "project_header", section });
        if (isCollapsed(section.key, section.defaultCollapsed)) break;
        pushRows(section.rows);
        break;
      case "shelf": {
        items.push({
          type: "group_header",
          key: section.key,
          parentKey: null,
          defaultCollapsed: section.defaultCollapsed,
        });
        if (isCollapsed(section.key, section.defaultCollapsed)) break;
        const page = shelfPage(section, shownPages(section.key));
        pushRows(page.rows);
        if (page.more > 0) items.push({ type: "show_more", shelfKey: section.key });
        break;
      }
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
    case "orphan":
      return navKey.orphan(item.session.id);
    case "task":
      return navKey.task(item.row.project.id, item.row.task.key);
    case "show_more":
      return navKey.more(item.shelfKey);
  }
}

/** Nav keys of the headers enclosing a task, innermost first. */
export function taskHeaderNavKeys(
  sections: SidebarSection[],
  projectId: string,
  taskKey: string,
): string[] {
  const isTask = (row: SidebarRow) =>
    row.kind === "task" && row.project.id === projectId && row.task.key === taskKey;
  for (const section of sections) {
    switch (section.kind) {
      case "project": {
        const group = section.groups.find((g) => g.rows.some(isTask));
        if (group) return [navKey.group(group.key), navKey.project(section.key)];
        break;
      }
      case "status":
        if (section.group.rows.some(isTask)) return [navKey.group(section.group.key)];
        break;
      case "folder":
        if (section.rows.some(isTask)) return [navKey.project(section.key)];
        break;
      case "shelf":
        if (section.rows.some(isTask)) return [navKey.group(section.key)];
        break;
    }
  }
  return [];
}

/**
 * Navigation IDs in sidebar display order, ignoring collapse state.
 * Unlinked sessions are bare session IDs, tasks are task workspace IDs.
 */
export function sidebarNavigationOrder(sections: SidebarSection[]): string[] {
  return sidebarNavItems(sections).flatMap((item) => {
    if (item.type === "orphan") return [item.session.id];
    if (item.type === "task") return [toTaskWorkspaceId(item.row.project.id, item.row.task.key)];
    return [];
  });
}
