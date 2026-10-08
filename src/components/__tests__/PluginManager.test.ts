import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";

const disabledPlugin = {
  id: "jira",
  name: "Jira",
  version: "0.1.0",
  host_api_version: "planeai.plugin-host.v1",
  source_kind: "builtin" as const,
  backend_entrypoint: "planeai-plugin-jira",
  ui_contributions: [
    {
      id: "settings",
      label: "Jira connection",
      placement: "preferences" as const,
      entrypoint: "jira-settings",
      order: null,
      shortcut: null,
    },
  ],
  installed_hash: null,
  installed_path: null,
  original_display_path: null,
  enabled: false,
  state: "disabled" as const,
  last_error: null,
  log_path: null,
};

const disabledGithubPlugin = {
  ...disabledPlugin,
  id: "github",
  name: "GitHub",
  source_kind: "local" as const,
  backend_entrypoint: "planeai-plugin-github",
  ui_contributions: [],
  installed_hash: "plugin-hash",
  installed_path: "/planeai/plugins/github",
  original_display_path: "/tmp/planeai-plugin-github",
};

const {
  list,
  enable,
  jiraMigrationStatus,
  migrateLegacyJira,
  githubMigrationStatus,
  migrateLegacyGithub,
} = vi.hoisted(() => ({
  list: vi.fn(),
  enable: vi.fn(),
  jiraMigrationStatus: vi.fn(),
  migrateLegacyJira: vi.fn(),
  githubMigrationStatus: vi.fn(),
  migrateLegacyGithub: vi.fn(),
}));

const { listen } = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("../../lib/api", () => ({
  plugins: {
    list,
    enable,
    jiraMigrationStatus,
    migrateLegacyJira,
    githubMigrationStatus,
    migrateLegacyGithub,
    disable: vi.fn(),
    reload: vi.fn(),
    installLocal: vi.fn(),
    removeLocal: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import PluginManager from "../PluginManager.svelte";

const noGithubMigration = {
  state: "not_needed" as const,
  legacy_detected: false,
  can_migrate: false,
  message: "No legacy GitHub pull request state was found.",
  error: null,
  imported_pull_requests: 0,
  skipped_state_only: 0,
  snapshot_path: null,
};

beforeEach(() => {
  listen.mockResolvedValue(vi.fn());
  jiraMigrationStatus.mockResolvedValue({
    state: "not_needed",
    legacy_detected: false,
    can_migrate: false,
    message: "No legacy Jira state was found.",
    error: null,
    imported_issues: 0,
    imported_links: 0,
    snapshot_path: null,
  });
  githubMigrationStatus.mockResolvedValue(noGithubMigration);
});

describe("PluginManager", () => {
  let target: HTMLElement;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(() => {
    if (component) unmount(component);
    target?.remove();
    vi.clearAllMocks();
  });

  it("publishes refreshed inventory after enabling a plugin", async () => {
    const runningPlugin = { ...disabledPlugin, enabled: true, state: "running" as const };
    list.mockResolvedValueOnce([disabledPlugin]).mockResolvedValueOnce([runningPlugin]);
    enable.mockResolvedValue(runningPlugin);
    const onInventoryChange = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginManager, { target, props: { onInventoryChange } });

    const enableButton = () =>
      Array.from(target.querySelectorAll<HTMLButtonElement>("button")).find((button) =>
        button.textContent?.includes("Enable"),
      );
    await vi.waitFor(() => expect(enableButton()).toBeDefined());
    enableButton()?.click();

    await vi.waitFor(() => expect(enable).toHaveBeenCalledWith("jira"));
    await vi.waitFor(() => expect(onInventoryChange).toHaveBeenLastCalledWith([runningPlugin]));
  });

  it("requires explicit confirmation and exposes retry for legacy Jira migration", async () => {
    const pending = {
      state: "available",
      legacy_detected: true,
      can_migrate: true,
      message: "Legacy Jira data is ready to migrate into the bundled Jira plugin.",
      error: null,
      imported_issues: 2,
      imported_links: 1,
      snapshot_path: "/tmp/legacy-jira-v1.json",
    };
    const failed = { ...pending, state: "failed", error: "simulated interruption" };
    list.mockResolvedValue([disabledPlugin]);
    jiraMigrationStatus.mockResolvedValueOnce(pending).mockResolvedValueOnce(failed);
    migrateLegacyJira.mockRejectedValueOnce(new Error("simulated interruption"));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginManager, { target });

    await vi.waitFor(() => expect(target.textContent).toContain("Migrate existing Jira state"));
    expect(target.textContent).toContain("Migrate and enable Jira plugin");
    expect(
      Array.from(target.querySelectorAll("button")).some(
        (button) => button.textContent?.trim() === "Enable",
      ),
    ).toBe(false);
    Array.from(target.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.includes("Migrate and enable"))
      ?.click();

    await vi.waitFor(() => expect(migrateLegacyJira).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(target.textContent).toContain("Retry migration"));
    expect(target.textContent).toContain("simulated interruption");
  });

  it("shows GitHub migration before the local plugin is installed and exposes retry errors", async () => {
    const pending = {
      state: "available" as const,
      legacy_detected: true,
      can_migrate: true,
      message: "Legacy GitHub pull request mappings are ready to migrate.",
      error: null,
      imported_pull_requests: 2,
      skipped_state_only: 1,
      snapshot_path: "/tmp/legacy-github-pr-v1.json",
    };
    const failed = { ...pending, state: "failed" as const, error: "simulated interruption" };
    list.mockResolvedValue([]);
    githubMigrationStatus.mockResolvedValueOnce(pending).mockResolvedValueOnce(failed);
    migrateLegacyGithub.mockRejectedValueOnce(new Error("simulated interruption"));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginManager, { target });

    await vi.waitFor(() =>
      expect(target.textContent).toContain("Migrate existing GitHub pull request state"),
    );
    expect(target.textContent).toContain(
      "Migration does not install or enable the local GitHub plugin.",
    );
    expect(target.textContent).toContain("Migrate GitHub pull request state");
    Array.from(target.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent?.includes("Migrate GitHub pull request state"))
      ?.click();

    await vi.waitFor(() => expect(migrateLegacyGithub).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(target.textContent).toContain("Retry GitHub migration"));
    expect(target.textContent).toContain("simulated interruption");
  });

  it("updates GitHub migration status from its change event without auto-enabling the plugin", async () => {
    const pending = {
      state: "available" as const,
      legacy_detected: true,
      can_migrate: true,
      message: "Legacy GitHub pull request mappings are ready to migrate.",
      error: null,
      imported_pull_requests: 1,
      skipped_state_only: 0,
      snapshot_path: "/tmp/legacy-github-pr-v1.json",
    };
    const completed = {
      ...pending,
      state: "completed" as const,
      can_migrate: false,
      message: "Imported 1 legacy GitHub pull request mapping.",
    };
    list.mockResolvedValue([disabledGithubPlugin]);
    githubMigrationStatus.mockResolvedValue(pending);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginManager, { target });

    await vi.waitFor(() => expect(target.textContent).toContain(pending.message));
    expect(
      Array.from(target.querySelectorAll("button")).some(
        (button) => button.textContent?.trim() === "Enable",
      ),
    ).toBe(false);
    await vi.waitFor(() =>
      expect(listen).toHaveBeenCalledWith("github-migration-changed", expect.any(Function)),
    );
    const listener = listen.mock.calls.find(
      ([event]) => event === "github-migration-changed",
    )?.[1] as (event: { payload: typeof completed }) => void;
    listener({ payload: completed });

    await vi.waitFor(() => expect(target.textContent).toContain(completed.message));
    expect(
      Array.from(target.querySelectorAll("button")).some(
        (button) => button.textContent?.trim() === "Enable",
      ),
    ).toBe(true);
    expect(enable).not.toHaveBeenCalled();
  });
});
