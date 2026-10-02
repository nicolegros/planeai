import type { LoopSessionItem } from "./types";
import { buildSidebarModel, type SidebarSection } from "./sidebar-model";
import { getSettings } from "./settings.svelte";
import * as projectStore from "./project-store.svelte";
import * as orchestrator from "./session-orchestrator.svelte";
import * as taskStore from "./task-store.svelte";
import * as loopStore from "./loop-store.svelte";

/** Loop child sessions keyed by loop ID, across all projects. Reactive when read inside $derived. */
export function currentLoopSessions(): Record<string, LoopSessionItem[]> {
  const map: Record<string, LoopSessionItem[]> = {};
  for (const project of projectStore.getProjects()) {
    for (const loop of loopStore.getLoopsForProject(project.id)) {
      const items = loopStore.getSessionsForLoop(loop.id);
      if (items.length > 0) map[loop.id] = items;
    }
  }
  return map;
}

/** The sidebar model built from the live stores and settings. Reactive when read inside $derived. */
export function currentSidebarModel(loopSessions = currentLoopSessions()): SidebarSection[] {
  const settings = getSettings();
  const projects = projectStore.getProjects();
  return buildSidebarModel({
    groupBy: settings.sidebar_group_by ?? "project",
    projects,
    sessions: orchestrator.getSessions(),
    tasksByProject: taskStore.getTasksByProject(),
    loopsByProject: Object.fromEntries(
      projects.map((p) => [p.id, loopStore.getLoopsForProject(p.id)]),
    ),
    loopSessionIds: new Set(
      Object.values(loopSessions).flatMap((items) => items.map((i) => i.session_id)),
    ),
    hideDoneTasks: !!settings.hide_done_tasks,
    hideEmptyProjects: !!settings.hide_empty_projects,
  });
}
