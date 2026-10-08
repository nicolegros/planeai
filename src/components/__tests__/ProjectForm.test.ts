import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, tick } from "svelte";

const { updateProject, createProject, validateGitRepo } = vi.hoisted(() => ({
  updateProject: vi.fn(() =>
    Promise.resolve({ id: "p1", name: "New project", path: "/projects/new", hidden: false }),
  ),
  createProject: vi.fn(() =>
    Promise.resolve({ id: "p2", name: "New project", path: "/projects/new", hidden: false }),
  ),
  validateGitRepo: vi.fn(() => Promise.resolve(true)),
}));

vi.mock("../../lib/api", () => ({
  projects: { validateGitRepo, update: updateProject, create: createProject },
  git: { cloneRepository: vi.fn() },
}));

vi.mock("../../lib/project-store.svelte", () => ({
  loadProjects: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("../../lib/settings.svelte", () => ({
  getSettings: () => ({ projects_base_path: "/projects" }),
}));

import ProjectForm from "../ProjectForm.svelte";

function renderForm() {
  const target = document.createElement("div");
  const onCreated = vi.fn();
  mount(ProjectForm, {
    target,
    props: {
      project: { id: "p1", name: "Old project", path: "/projects/old", hidden: false },
      onCreated,
      onCancel: vi.fn(),
    },
  });
  return { target, onCreated };
}

describe("ProjectForm editing", () => {
  beforeEach(() => {
    updateProject.mockClear();
    validateGitRepo.mockClear();
  });

  it("prefills the selected project's name and path", () => {
    const { target } = renderForm();

    expect(target.querySelector<HTMLInputElement>("[data-field='path'] input")?.value).toBe(
      "/projects/old",
    );
    expect(target.querySelector<HTMLInputElement>("[data-field='name'] input")?.value).toBe(
      "Old project",
    );
    expect(target.querySelector<HTMLButtonElement>("button[type='submit']")?.textContent).toContain(
      "Save project",
    );
  });

  it("validates and persists renamed project details", async () => {
    const { target, onCreated } = renderForm();
    const pathInput = target.querySelector<HTMLInputElement>("[data-field='path'] input")!;
    const nameInput = target.querySelector<HTMLInputElement>("[data-field='name'] input")!;

    pathInput.value = "/projects/new";
    pathInput.dispatchEvent(new Event("input", { bubbles: true }));
    nameInput.value = "New project";
    nameInput.dispatchEvent(new Event("input", { bubbles: true }));
    target.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true }));
    await tick();
    flushSync();

    await vi.waitFor(() => expect(onCreated).toHaveBeenCalledOnce());

    expect(validateGitRepo).toHaveBeenCalledWith("/projects/new");
    expect(updateProject).toHaveBeenCalledWith("p1", "New project", "/projects/new");
  });
});
