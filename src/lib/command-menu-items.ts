/**
 * Command menu items — pure derivations for the root Cmd+K list.
 *
 * The root list searches *tasks* rather than sessions: selecting a task navigates to its
 * workspace exactly as the sidebar does, landing in the linked session when one exists.
 * Sessions therefore only need listing when no visible task can route to them.
 */
import type { Project, Session, TaskItem } from "./types";

/** Canonical task status order, matching the sidebar and task panel. */
const STATUS_ORDER = ["in_progress", "in_review", "todo", "done"] as const;

/** A task paired with the project coordinates needed to navigate to its workspace. */
export interface CommandMenuTaskRow {
  task: TaskItem;
  repoPath: string;
  projectId: string;
}

function statusRank(status: string): number {
  const index = STATUS_ORDER.indexOf(status as (typeof STATUS_ORDER)[number]);
  // Unknown statuses sort after the known ones rather than being dropped.
  return index === -1 ? STATUS_ORDER.length : index;
}

/**
 * Flattens every project's tasks into a single searchable list.
 *
 * Tasks from `activeProjectPath` come first so the project you are working in wins ties,
 * then each project's tasks are ordered by status and descending priority. Hidden projects
 * are included: hiding declutters the sidebar, it does not remove a task from search.
 */
export function computeCommandMenuTaskRows(
  projects: Project[],
  tasksByProject: Record<string, TaskItem[]>,
  activeProjectPath: string | null,
  hideDone: boolean,
): CommandMenuTaskRow[] {
  const ordered = activeProjectPath
    ? [
        ...projects.filter((project) => project.path === activeProjectPath),
        ...projects.filter((project) => project.path !== activeProjectPath),
      ]
    : projects;

  const rows: CommandMenuTaskRow[] = [];
  for (const project of ordered) {
    const tasks = (tasksByProject[project.path] ?? [])
      .filter((task) => !(hideDone && task.status === "done"))
      .slice()
      .sort((a, b) => statusRank(a.status) - statusRank(b.status) || b.priority - a.priority);
    for (const task of tasks) {
      rows.push({ task, repoPath: project.path, projectId: project.id });
    }
  }
  return rows;
}

/**
 * Separates the matchable part of a `Command.Item` value from its disambiguating suffix.
 *
 * bits-ui scores the query against `value + keywords`, so anything placed in the value becomes
 * searchable. Task values need the repo path to stay unique, but the repo path must not be
 * matchable — a NUL separator keeps the two concerns apart.
 */
export const VALUE_SEPARATOR = "\u0000";

/**
 * Stable, unique `Command.Item` value for a task row.
 *
 * Task keys are only unique per task project (`{prefix}-{seq}`), so the repo path is appended
 * after the separator to keep identical keys from different projects distinct. Only the key and
 * title precede the separator, so only those two are searchable.
 */
export function taskRowValue(row: CommandMenuTaskRow): string {
  return `${row.task.key} ${row.task.title}${VALUE_SEPARATOR}${row.repoPath}`;
}

/** Strips the disambiguating suffix, leaving only the text a query should match against. */
export function matchableValue(value: string): string {
  const index = value.indexOf(VALUE_SEPARATOR);
  return index === -1 ? value : value.slice(0, index);
}

/**
 * Sessions that need their own command menu entry because no visible task row reaches them.
 *
 * That means: sessions with no task, sessions whose task is missing from the visible rows
 * (deleted, failed to load, or hidden by `hide_done_tasks`), and loop sessions — whose role
 * and round identity a task entry cannot express.
 */
export function computeCommandMenuSessions(
  sessions: Session[],
  taskRows: CommandMenuTaskRow[],
  isLoopSession: (sessionId: string) => boolean,
): Session[] {
  const routable = new Set(taskRows.map((row) => `${row.projectId}\u0000${row.task.key}`));
  return sessions.filter((session) => {
    if (isLoopSession(session.id)) return true;
    if (!session.task_key) return true;
    return !routable.has(`${session.project_id}\u0000${session.task_key}`);
  });
}
