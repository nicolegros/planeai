import { open } from "@tauri-apps/plugin-dialog";
import { projects as projectsApi } from "./api";
import * as projectStore from "./project-store.svelte";
import { getSettings } from "./settings.svelte";
import { showSnackbar } from "./snackbar.svelte";
import * as taskStore from "./task-store.svelte";
import type { Project } from "./types";

/** Asks for the folder a missing project moved to and relinks the project to it. Returns the relinked project, or null when cancelled or refused. */
export async function locateProjectFolder(project: Project): Promise<Project | null> {
  try {
    const parent = await projectsApi.existingParentDir(project.path);
    const picked = await open({
      directory: true,
      multiple: false,
      defaultPath: parent ?? getSettings().projects_base_path ?? undefined,
    });
    if (typeof picked !== "string") return null;
    await projectStore.updateProject(project.id, project.name, picked);
    // Tasks are keyed by project path.
    await taskStore.refresh(projectStore.getProjects().map((candidate) => candidate.path));
    showSnackbar(
      "Project relinked. Agent conversations started in the old folder may not resume.",
      "info",
    );
    return projectStore.getProject(project.id) ?? null;
  } catch (error) {
    showSnackbar(String(error));
    return null;
  }
}
