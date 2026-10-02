import type { LoopSessionItem } from "./types";
import { buildSidebarModel, resolveSidebarGroupBy, type SidebarSection } from "./sidebar-model";
import { getSettings } from "./settings.svelte";
import * as projectStore from "./project-store.svelte";
import * as orchestrator from "./session-orchestrator.svelte";
import * as taskStore from "./task-store.svelte";
import * as loopStore from "./loop-store.svelte";

// Built once and shared by every reader (App session cycling, UnifiedSidebar).
const model = $derived.by((): SidebarSection[] => {
  const settings = getSettings();
  const projects = projectStore.getProjects();
  const loopsByProject = Object.fromEntries(
    projects.map((p) => [p.id, loopStore.getLoopsForProject(p.id)]),
  );
  const loopSessions: Record<string, LoopSessionItem[]> = {};
  for (const loop of Object.values(loopsByProject).flat()) {
    loopSessions[loop.id] = loopStore.getSessionsForLoop(loop.id);
  }
  return buildSidebarModel({
    groupBy: resolveSidebarGroupBy(settings),
    projects,
    sessions: orchestrator.getSessions(),
    tasksByProject: taskStore.getTasksByProject(),
    loopsByProject,
    loopSessions,
    hideDoneTasks: !!settings.hide_done_tasks,
    hideEmptyProjects: !!settings.hide_empty_projects,
  });
});

/** The sidebar model built from the live stores and settings. Reactive when read inside $derived. */
export function getSidebarModel(): SidebarSection[] {
  return model;
}
