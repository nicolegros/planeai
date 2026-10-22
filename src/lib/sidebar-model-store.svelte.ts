import { buildSidebarModel, resolveSidebarGroupBy, type SidebarSection } from "./sidebar-model";
import { getSettings } from "./settings.svelte";
import * as projectStore from "./project-store.svelte";
import * as orchestrator from "./session-orchestrator.svelte";
import * as taskStore from "./task-store.svelte";

// Built once and shared by every reader (App session cycling, UnifiedSidebar).
const model = $derived.by((): SidebarSection[] => {
  const settings = getSettings();
  return buildSidebarModel({
    groupBy: resolveSidebarGroupBy(settings),
    projects: projectStore.getProjects(),
    sessions: orchestrator.getSessions(),
    tasksByProject: taskStore.getTasksByProject(),
    hideDoneTasks: !!settings.hide_done_tasks,
    hideEmptyProjects: !!settings.hide_empty_projects,
  });
});

/** The sidebar model built from the live stores and settings. Reactive when read inside $derived. */
export function getSidebarModel(): SidebarSection[] {
  return model;
}
