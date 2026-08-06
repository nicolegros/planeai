/**
 * Global project store — single source of truth for project state.
 * Coordinates with session orchestrator on delete/archive.
 */
import { projects as projectsApi } from "./api";
import type { Project } from "./types";
import { removeProjectSessions } from "./session-orchestrator.svelte";

let projects = $state<Project[]>([]);

export function getProjects(): Project[] {
  return projects;
}

export function getProject(id: string): Project | undefined {
  return projects.find((p) => p.id === id);
}

export async function loadProjects(): Promise<void> {
  projects = await projectsApi.list();
}

export async function createProject(name: string, path: string): Promise<void> {
  await projectsApi.create(name, path);
  await loadProjects();
}

export async function archiveProject(id: string): Promise<void> {
  await projectsApi.archive(id);
  removeProjectSessions(id);
  projects = projects.filter((p) => p.id !== id);
}

export async function hideProject(id: string): Promise<void> {
  await projectsApi.hide(id);
  projects = projects.map((project) =>
    project.id === id ? { ...project, hidden: true } : project,
  );
}

export async function unhideProject(id: string): Promise<void> {
  await projectsApi.unhide(id);
  projects = projects.map((project) =>
    project.id === id ? { ...project, hidden: false } : project,
  );
}

export async function deleteProject(id: string): Promise<void> {
  await projectsApi.delete(id);
  removeProjectSessions(id);
  projects = projects.filter((p) => p.id !== id);
}

export async function restoreProject(id: string): Promise<void> {
  await projectsApi.restore(id);
  await loadProjects();
}
