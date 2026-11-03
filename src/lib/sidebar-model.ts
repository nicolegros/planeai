/**
 * Sidebar model: the single source of the sidebar's sections and row order.
 * Rendering, keyboard navigation and session cycling all derive from it so they never drift apart.
 */
import { sessionTaskProjectId, type Project, type Session, type TaskItem } from "./types";
import { shouldHideProject, toTaskWorkspaceId } from "./sidebar-session-order";
import { isTaskSnoozed } from "./task-snooze";

export type SidebarGroupBy = "project" | "status";
export type SidebarLayout = "classic" | "cards";

export function resolveSidebarLayout(settings: {
  sidebar_layout?: SidebarLayout | null;
}): SidebarLayout {
  return settings.sidebar_layout ?? "cards";
}

/** Project labels only apply where rows are not already under their project: Cards, or Classic grouped by status. */
export function projectLabelsApply(
  settings: Parameters<typeof resolveSidebarLayout>[0] &
    Parameters<typeof resolveSidebarGroupBy>[0],
): boolean {
  return resolveSidebarLayout(settings) === "cards" || resolveSidebarGroupBy(settings) === "status";
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

/** Where a task sits in the sidebar: with its status, or set aside until it wakes. */
export type TaskLane = TaskStatus | "snoozed";

type RowOrder = "priority" | "wake";

interface Lane {
  label: string;
  dotClass: string;
  defaultCollapsed: boolean;
  order: RowOrder;
}

export const LANES: Record<TaskLane, Lane> = {
  in_progress: {
    label: TASK_STATUS_LABELS.in_progress,
    dotClass: "bg-status-running",
    defaultCollapsed: false,
    order: "priority",
  },
  in_review: {
    label: TASK_STATUS_LABELS.in_review,
    dotClass: "bg-status-review",
    defaultCollapsed: false,
    order: "priority",
  },
  todo: {
    label: TASK_STATUS_LABELS.todo,
    dotClass: "bg-t3",
    defaultCollapsed: false,
    order: "priority",
  },
  snoozed: { label: "Snoozed", dotClass: "bg-t3", defaultCollapsed: true, order: "wake" },
  done: {
    label: TASK_STATUS_LABELS.done,
    dotClass: "bg-status-running",
    defaultCollapsed: true,
    order: "priority",
  },
};

/** Classic group order: per project the snoozed group follows every status group; across projects it precedes Done. */
const PROJECT_GROUP_LANES: TaskLane[] = ["in_progress", "in_review", "todo", "done", "snoozed"];
const STATUS_SECTION_LANES: TaskLane[] = ["in_progress", "in_review", "todo", "snoozed", "done"];

export function taskLane(task: TaskItem, now: number): TaskLane {
  return isTaskSnoozed(task, now) ? "snoozed" : normalizeTaskStatus(task.status);
}

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

/** Cards keeps `in_review` tasks on top with unlinked sessions; every other lane gets a shelf. */
export type ShelfLane = Exclude<TaskLane, "in_review">;

export interface Shelf {
  variant: "card" | "slim";
  /** Rows shown per "Show more" step; null shows every row. */
  pageSize: number | null;
  /** The task open in the main pane stays listed while the shelf is collapsed. */
  keepsOpenTaskVisible: boolean;
}

/** Cards shelves in display order. Their labels and collapse defaults are in LANES. */
export const SHELVES: Record<ShelfLane, Shelf> = {
  in_progress: { variant: "card", pageSize: null, keepsOpenTaskVisible: false },
  todo: { variant: "slim", pageSize: null, keepsOpenTaskVisible: false },
  snoozed: { variant: "slim", pageSize: null, keepsOpenTaskVisible: true },
  done: { variant: "slim", pageSize: 25, keepsOpenTaskVisible: false },
};

export interface SidebarLaneGroup {
  key: string;
  lane: TaskLane;
  rows: SidebarTaskRow[];
  defaultCollapsed: boolean;
}

export interface SidebarProjectSection {
  kind: "project";
  key: string;
  project: Project;
  orphans: SidebarSessionRow[];
  groups: SidebarLaneGroup[];
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
  lane: ShelfLane;
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
  | { kind: "status"; key: string; group: SidebarLaneGroup; defaultCollapsed: boolean }
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
  /** The clock snoozes are read against. */
  now: number;
}

function normalizeTaskStatus(status: string): TaskStatus {
  return (TASK_STATUSES as readonly string[]).includes(status) ? (status as TaskStatus) : "todo";
}

const keyCollator = new Intl.Collator(undefined, { numeric: true });

const ROW_ORDERS: Record<RowOrder, (a: SidebarTaskRow, b: SidebarTaskRow) => number> = {
  priority: (a, b) => b.task.priority - a.task.priority,
  // Snoozed rows always have a wake time.
  wake: (a, b) => Date.parse(a.task.snoozed_until!) - Date.parse(b.task.snoozed_until!),
};

/** Rows of one lane in sidebar order. */
function rowsInLane(
  rows: SidebarTaskRow[],
  lane: TaskLane,
  projectOrder: Map<string, number>,
  now: number,
): SidebarTaskRow[] {
  const order = ROW_ORDERS[LANES[lane].order];
  return rows
    .filter((row) => taskLane(row.task, now) === lane)
    .sort(
      (a, b) =>
        order(a, b) ||
        (projectOrder.get(a.project.id) ?? 0) - (projectOrder.get(b.project.id) ?? 0) ||
        keyCollator.compare(a.task.key, b.task.key),
    );
}

function groupRows(
  rows: SidebarTaskRow[],
  lanes: TaskLane[],
  projectOrder: Map<string, number>,
  hideDoneTasks: boolean,
  now: number,
  keyFor: (lane: TaskLane) => string,
): SidebarLaneGroup[] {
  const groups: SidebarLaneGroup[] = [];
  for (const lane of lanes) {
    if (lane === "done" && hideDoneTasks) continue;
    const laneRows = rowsInLane(rows, lane, projectOrder, now);
    if (laneRows.length === 0) continue;
    groups.push({
      key: keyFor(lane),
      lane,
      rows: laneRows,
      defaultCollapsed: LANES[lane].defaultCollapsed,
    });
  }
  return groups;
}

export function buildSidebarModel(input: SidebarModelInput): SidebarSection[] {
  const { layout, groupBy, sessions, tasksByProject, hideDoneTasks, hideEmptyProjects, now } =
    input;
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
          (row) => !(hideDoneTasks && taskLane(row.task, now) === "done"),
        ),
      }))
      .filter((p) => !shouldHideProject(p.orphans.length, p.tasks.length, hideEmptyProjects));
    // Unlinked sessions keep their order and come before the tasks waiting for review.
    const topRows = (orphans: SidebarSessionRow[], tasks: SidebarTaskRow[]): SidebarRow[] => [
      ...orphans,
      ...rowsInLane(tasks, "in_review", projectOrder, now),
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
    for (const lane of Object.keys(SHELVES) as ShelfLane[]) {
      const rows = rowsInLane(allTasks, lane, projectOrder, now);
      if (rows.length === 0) continue;
      sections.push({
        kind: "shelf",
        key: `shelf:${lane}`,
        lane,
        rows,
        defaultCollapsed: LANES[lane].defaultCollapsed,
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
      STATUS_SECTION_LANES,
      projectOrder,
      hideDoneTasks,
      now,
      // The prefix predates lanes; saved collapse state is keyed by it.
      (lane) => `status:${lane}`,
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
      PROJECT_GROUP_LANES,
      projectOrder,
      hideDoneTasks,
      now,
      (lane) => `project-status:${project.id}:${lane}`,
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

/** Whether a row is the task open in the main pane. */
export type IsOpenTask = (row: SidebarTaskRow) => boolean;

/**
 * The rows a shelf shows, and how many the next "Show more" adds. A collapsed shelf shows none, or
 * only the open task where the shelf keeps it visible.
 */
export function shelfPage(
  shelf: SidebarShelfSection,
  pages: number,
  collapsed = false,
  isOpen: IsOpenTask = () => false,
): { rows: SidebarTaskRow[]; more: number } {
  const { pageSize, keepsOpenTaskVisible } = SHELVES[shelf.lane];
  if (collapsed) return { rows: keepsOpenTaskVisible ? shelf.rows.filter(isOpen) : [], more: 0 };
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
  isOpen: IsOpenTask = () => false,
): SidebarNavItem[] {
  const items: SidebarNavItem[] = [];
  const pushRows = (rows: SidebarRow[]) => {
    for (const row of rows) {
      items.push(
        row.kind === "task" ? { type: "task", row } : { type: "orphan", session: row.session },
      );
    }
  };
  const pushGroup = (group: SidebarLaneGroup, parentKey: string | null) => {
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
        const collapsed = isCollapsed(section.key, section.defaultCollapsed);
        const page = shelfPage(section, shownPages(section.key), collapsed, isOpen);
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
 * Navigation IDs in sidebar display order, by default ignoring collapse state.
 * Unlinked sessions are bare session IDs, tasks are task workspace IDs.
 */
export function sidebarNavigationOrder(
  sections: SidebarSection[],
  isCollapsed?: IsCollapsed,
  shownPages?: ShownPages,
): string[] {
  return sidebarNavItems(sections, isCollapsed, shownPages).flatMap((item) => {
    if (item.type === "orphan") return [item.session.id];
    if (item.type === "task") return [toTaskWorkspaceId(item.row.project.id, item.row.task.key)];
    return [];
  });
}
