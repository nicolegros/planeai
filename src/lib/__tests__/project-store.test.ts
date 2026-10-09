import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Project } from "../types";

const { list, update, existingParentDir, focusHandlers, open, showSnackbar } = vi.hoisted(() => ({
  list: vi.fn(),
  update: vi.fn(),
  existingParentDir: vi.fn(),
  focusHandlers: [] as Array<() => void>,
  open: vi.fn(),
  showSnackbar: vi.fn(),
}));

vi.mock("../api", () => ({
  projects: { list, update, existingParentDir },
  appWindow: {
    onFocused: (handler: () => void) => {
      focusHandlers.push(handler);
      return () => focusHandlers.splice(focusHandlers.indexOf(handler), 1);
    },
  },
}));
vi.mock("../session-orchestrator.svelte", () => ({ removeProjectSessions: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));
vi.mock("../settings.svelte", () => ({ getSettings: () => ({ projects_base_path: "/base" }) }));
vi.mock("../snackbar.svelte", () => ({ showSnackbar }));

import * as projectStore from "../project-store.svelte";
import { locateProjectFolder } from "../locate-project";

const project = (overrides: Partial<Project> = {}): Project => ({
  id: "p1",
  name: "app",
  path: "/old/app",
  hidden: false,
  path_missing: false,
  ...overrides,
});

describe("project store", () => {
  beforeEach(() => {
    list.mockReset();
    update.mockReset();
    focusHandlers.length = 0;
  });

  it("reloads projects when the window regains focus, without overlapping loads", async () => {
    let resolveLoad: (projects: Project[]) => void = () => {};
    list.mockImplementation(() => new Promise<Project[]>((resolve) => (resolveLoad = resolve)));
    const stop = projectStore.reloadProjectsOnFocus();

    focusHandlers[0]();
    focusHandlers[0]();
    resolveLoad([project({ path_missing: true })]);
    await vi.waitFor(() => expect(projectStore.getProject("p1")?.path_missing).toBe(true));

    expect(list).toHaveBeenCalledTimes(1);
    stop();
    expect(focusHandlers).toHaveLength(0);
  });
});

describe("locating a moved project folder", () => {
  beforeEach(() => {
    list.mockReset();
    update.mockReset();
    open.mockReset();
    showSnackbar.mockReset();
    existingParentDir.mockReset();
  });

  it("starts in the old parent folder and relinks to the picked folder", async () => {
    existingParentDir.mockResolvedValue("/old");
    open.mockResolvedValue("/new/app");
    update.mockResolvedValue(project({ path: "/new/app" }));
    list.mockResolvedValue([project({ path: "/new/app" })]);

    await locateProjectFolder(project({ path_missing: true }));

    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, defaultPath: "/old" });
    expect(update).toHaveBeenCalledWith("p1", "app", "/new/app");
    expect(projectStore.getProject("p1")?.path).toBe("/new/app");
    expect(showSnackbar).toHaveBeenCalledWith(
      "Project relinked. Agent conversations started in the old folder may not resume.",
      "info",
    );
  });

  it("falls back to the projects folder and shows the backend refusal", async () => {
    existingParentDir.mockResolvedValue(null);
    open.mockResolvedValue("/elsewhere/clone");
    update.mockRejectedValue("Could not reattach worktree sessions to this folder: fix login.");

    await locateProjectFolder(project({ path_missing: true }));

    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, defaultPath: "/base" });
    expect(showSnackbar).toHaveBeenCalledWith(
      "Could not reattach worktree sessions to this folder: fix login.",
    );
  });
});
