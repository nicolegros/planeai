/**
 * Sidebar navigation ID helpers. The display order itself lives in sidebar-model.ts.
 *
 * Loop dashboards are "loop:<id>" entries; regular sessions remain bare IDs.
 */

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

/** Create a task workspace ID for navigation and MRU state. */
export function toTaskWorkspaceId(projectId: string, taskKey: string): string {
  return `task:${projectId}:${taskKey}`;
}

/** Detect a task workspace navigation ID. */
export function isTaskWorkspaceId(id: string): boolean {
  return id.startsWith("task:");
}

/** Parse a task workspace navigation ID. */
export function parseTaskWorkspaceId(id: string): { projectId: string; taskKey: string } | null {
  const [, projectId, ...keyParts] = id.split(":");
  const taskKey = keyParts.join(":");
  return projectId && taskKey ? { projectId, taskKey } : null;
}
