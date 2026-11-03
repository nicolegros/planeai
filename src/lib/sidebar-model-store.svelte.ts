import {
  buildSidebarModel,
  resolveSidebarGroupBy,
  resolveSidebarLayout,
  sidebarNavigationOrder,
  type SidebarSection,
} from "./sidebar-model";
import { getCollapsedSections, setCollapsedSections } from "./layout-state";
import { getSettings } from "./settings.svelte";
import * as projectStore from "./project-store.svelte";
import * as orchestrator from "./session-orchestrator.svelte";
import * as taskStore from "./task-store.svelte";

// Built once and shared by every reader (App session cycling, UnifiedSidebar).
const model = $derived.by((): SidebarSection[] => {
  const settings = getSettings();
  return buildSidebarModel({
    layout: resolveSidebarLayout(settings),
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

// Only sections the user set away from their default are stored.
let collapsedSections = $state<Record<string, boolean>>(getCollapsedSections());
// Open "Show more" pages per shelf, for this app run only.
let shelfPages = $state<Record<string, number>>({});

export function isCollapsed(key: string, fallback: boolean): boolean {
  return collapsedSections[key] ?? fallback;
}

export function setCollapsed(key: string, collapsed: boolean, fallback: boolean): void {
  if (isCollapsed(key, fallback) === collapsed) return;
  const { [key]: _previous, ...rest } = collapsedSections;
  collapsedSections = collapsed === fallback ? rest : { ...rest, [key]: collapsed };
  setCollapsedSections(collapsedSections);
}

export function shownPages(shelfKey: string): number {
  return shelfPages[shelfKey] ?? 1;
}

export function showMore(shelfKey: string): void {
  shelfPages = { ...shelfPages, [shelfKey]: shownPages(shelfKey) + 1 };
}

const visibleNavigationOrder = $derived(sidebarNavigationOrder(model, isCollapsed, shownPages));

/** Task workspace and session IDs of the rows the sidebar shows, top to bottom. */
export function getVisibleNavigationOrder(): string[] {
  return visibleNavigationOrder;
}

/** Reload collapse state from storage and close every shelf page. */
export function _resetForTests(): void {
  collapsedSections = getCollapsedSections();
  shelfPages = {};
}
