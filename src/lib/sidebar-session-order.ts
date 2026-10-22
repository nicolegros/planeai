/**
 * Sidebar navigation ID helpers. The display order itself lives in sidebar-model.ts.
 */

/**
 * Pure predicate: should a project be hidden from the sidebar?
 * Hidden when hideEmpty is on AND the project has zero visible items.
 */
export function shouldHideProject(
  orphanCount: number,
  visibleTaskCount: number,
  hideEmpty: boolean,
): boolean {
  return hideEmpty && orphanCount === 0 && visibleTaskCount === 0;
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
