/**
 * Sidebar session order — computes session IDs in the same order as the sidebar display.
 * Shared between App.svelte (for keyboard navigation) and UnifiedSidebar (for rendering).
 *
 * Loop dashboards are included as "loop:<id>" entries; regular sessions remain bare IDs.
 */
import type { Session, Project, TaskItem, LoopRunSummary, LoopSessionItem } from "./types";

const STATUS_ORDER = ["in_progress", "in_review", "todo", "done"] as const;

/**
 * Pure predicate: should a project be hidden from the sidebar?
 * Hidden when hideEmpty is on AND the project has zero visible items.
 */
export function shouldHideProject(
  orphanCount: number,
  visibleTaskCount: number,
  hideEmpty: boolean,
  loopCount: number = 0,
): boolean {
  return hideEmpty && orphanCount === 0 && visibleTaskCount === 0 && loopCount === 0;
}

/** Detect whether a cycle/MRU item is a loop dashboard vs a session. */
export function isLoopId(id: string): boolean {
  return id.startsWith("loop:");
}

/** Extract the raw loop ID from a prefixed string. */
export function parseLoopId(id: string): string {
  return id.slice(5);
}

/** Create a prefixed loop ID for use in cycle lists. */
export function toLoopId(loopId: string): string {
  return `loop:${loopId}`;
}

export function computeSidebarSessionOrder(
  projects: Project[],
  sessions: Session[],
  tasksByProject: Record<string, TaskItem[]>,
  hideDone: boolean,
  loopsByProject?: Record<string, LoopRunSummary[]>,
  loopSessions?: Record<string, LoopSessionItem[]>,
  loopSessionIds?: Set<string>,
): string[] {
  const allTaskKeys = new Set(
    Object.values(tasksByProject)
      .flat()
      .map((t) => t.key),
  );
  const ids: string[] = [];
  const loopSessionSet = loopSessionIds ?? new Set<string>();

  for (const project of projects) {
    if (project.hidden) continue;

    // Loops first (with their child sessions)
    const projectLoops = loopsByProject?.[project.id] ?? [];
    for (const loop of projectLoops) {
      ids.push(toLoopId(loop.id));
      // Include child sessions in order
      const children = loopSessions?.[loop.id] ?? [];
      for (const child of children) {
        ids.push(child.session_id);
      }
    }

    // Orphan sessions (no task_key or task not found, and not in a loop)
    for (const s of sessions) {
      if (
        s.project_id === project.id &&
        (!s.task_key || !allTaskKeys.has(s.task_key)) &&
        !loopSessionSet.has(s.id)
      ) {
        ids.push(s.id);
      }
    }
    // Task-linked sessions in status/priority order
    const projectTasks = tasksByProject[project.path] ?? [];
    for (const status of STATUS_ORDER) {
      if (status === "done" && hideDone) continue;
      const group = projectTasks
        .filter((t) => t.status === status)
        .sort((a, b) => b.priority - a.priority);
      for (const t of group) {
        const linked = sessions.find((s) => s.task_key === t.key);
        if (linked) ids.push(linked.id);
      }
    }
  }

  return ids;
}
