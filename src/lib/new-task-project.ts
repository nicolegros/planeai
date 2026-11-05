import type { Project } from "./types";

/** The project a new task starts in: the focused task's, then the active session's, then the first one. */
export function newTaskProjectPath(
  focusedTaskProject: Project | null | undefined,
  activeSessionProjectId: string | null | undefined,
  projects: readonly Project[],
): string {
  return (
    focusedTaskProject?.path ??
    projects.find((project) => project.id === activeSessionProjectId)?.path ??
    projects[0]?.path ??
    ""
  );
}
