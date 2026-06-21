/**
 * Sidebar session order — computes session IDs in the same order as the sidebar display.
 * Shared between App.svelte (for keyboard navigation) and UnifiedSidebar (for rendering).
 */
import type { Session, Project, TaskItem } from "./types";
import { getSettings } from "./settings.svelte";

const STATUS_ORDER = ["in_progress", "in_review", "todo", "done"] as const;

export function computeSidebarSessionOrder(
  projects: Project[],
  sessions: Session[],
  tasksByProject: Record<string, TaskItem[]>,
): string[] {
  const allTaskKeys = new Set(Object.values(tasksByProject).flat().map(t => t.key));
  const hideDone = getSettings().hide_done_tasks;
  const ids: string[] = [];

  for (const project of projects) {
    // Orphan sessions (no task_key or task not found)
    for (const s of sessions) {
      if (s.project_id === project.id && (!s.task_key || !allTaskKeys.has(s.task_key))) {
        ids.push(s.id);
      }
    }
    // Task-linked sessions in status/priority order
    const projectTasks = tasksByProject[project.path] ?? [];
    for (const status of STATUS_ORDER) {
      if (status === "done" && hideDone) continue;
      const group = projectTasks.filter(t => t.status === status).sort((a, b) => b.priority - a.priority);
      for (const t of group) {
        const linked = sessions.find(s => s.task_key === t.key);
        if (linked) ids.push(linked.id);
      }
    }
  }

  return ids;
}
