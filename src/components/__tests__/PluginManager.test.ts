import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import type {
  DiscoveryCardState,
  PluginDiscoveryCandidate,
  StagedPluginInstall,
} from "../../lib/types";

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
  remote_origin: null,
  tools: [],
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
  remote_origin: null,
};

const remoteAcmePlugin = {
  ...disabledGithubPlugin,
  id: "acme",
  name: "Acme",
  version: "1.2.0",
  backend_entrypoint: "planeai-plugin-acme",
  installed_hash: "acme-hash",
  installed_path: "/planeai/plugins/acme",
  original_display_path: "/planeai/plugins/staging/acme",
  remote_origin: {
    repo: "acme/planeai-plugin-acme",
    release_tag: "v1.2.0",
    source_url:
      "https://github.com/acme/planeai-plugin-acme/releases/download/v1.2.0/planeai-plugin-acme-macos-arm64.tar.gz",
    archive_sha256: "acme-archive-sha",
  },
};

const stagedAcme: StagedPluginInstall = {
  staging_id: "stage-1",
  repo: "acme/planeai-plugin-acme",
  source_url: remoteAcmePlugin.remote_origin.source_url,
  release_tag: "v1.2.0",
  integrity: { kind: "verified", sha256: "acme-archive-sha" },
  manifest: {
    id: "acme",
    name: "Acme",
    version: "1.2.0",
    host_api_version: "planeai.plugin-host.v6",
    capabilities: ["settings"],
    ui_contributions: [],
    tools: [],
  },
  replaces: { kind: "none" },
  version_mismatch: null,
};

function candidate(fullName: string): PluginDiscoveryCandidate {
  return {
    full_name: fullName,
    description: null,
    url: `https://github.com/${fullName}`,
    updated_at: "2026-11-01T00:00:00Z",
    stargazers_count: 3,
    language: "Rust",
  };
}

const {
  list,
  enable,
  jiraMigrationStatus,
  migrateLegacyJira,
  githubMigrationStatus,
  migrateLegacyGithub,
  discover,
  resolveCandidate,
  stageRemote,
  commitStaged,
  discardStaged,
} = vi.hoisted(() => ({
  list: vi.fn(),
  enable: vi.fn(),
  jiraMigrationStatus: vi.fn(),
  migrateLegacyJira: vi.fn(),
  githubMigrationStatus: vi.fn(),
  migrateLegacyGithub: vi.fn(),
  discover: vi.fn(),
  resolveCandidate: vi.fn(),
  stageRemote: vi.fn(),
  commitStaged: vi.fn(),
  discardStaged: vi.fn(),
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
    discover,
    resolveCandidate,
    stageRemote,
    commitStaged,
    discardStaged,
    disable: vi.fn(),
    reload: vi.fn(),
    installLocal: vi.fn(),
    removeLocal: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

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

  const buttons = (root: ParentNode) =>
    Array.from(root.querySelectorAll<HTMLButtonElement>("button"));
  const buttonLabeled = (root: ParentNode, label: string) =>
    buttons(root).find((button) => button.textContent?.trim() === label);
  const card = (repo: string) =>
    Array.from(target.querySelectorAll("article")).find(
      (article) => article.querySelector("h4")?.textContent?.trim() === repo,
    );
  const cardButtons = (repo: string) =>
    buttons(card(repo)!).map((button) => button.textContent?.trim());
  const reviewDialog = () => document.querySelector<HTMLElement>('[role="dialog"]');
  const paragraphs = () =>
    Array.from(target.querySelectorAll("p")).map((line) => line.textContent?.trim() ?? "");

  async function mountWithDiscovery(candidates: PluginDiscoveryCandidate[]) {
    discover.mockResolvedValue(candidates);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginManager, { target });
    await vi.waitFor(() => expect(buttonLabeled(target, "Discover plugins")).toBeDefined());
    buttonLabeled(target, "Discover plugins")?.click();
  }

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

  it("renders each Discovery card action from its resolved state", async () => {
    const states: Record<string, DiscoveryCardState> = {
      "acme/install": { kind: "install", latest_version: "1.2.0" },
      "acme/current": { kind: "up_to_date", installed_version: "1.0.0" },
      "acme/update": { kind: "update", installed_version: "1.0.0", latest_version: "2.0.0" },
      "acme/reinstall": { kind: "reinstall", installed_version: "1.0.0" },
      "acme/local": {
        kind: "installed_locally",
        plugin_id: "acme",
        installed_version: "0.9.0",
        latest_version: "1.2.0",
      },
      "acme/unavailable": {
        kind: "unavailable",
        reason: "Not available for macos-arm64: the latest release ships no matching asset.",
      },
    };
    let settlePending: (state: DiscoveryCardState) => void = () => {};
    const pending = new Promise<DiscoveryCardState>((resolve) => {
      settlePending = resolve;
    });
    resolveCandidate.mockImplementation((repo: string) => {
      if (repo === "acme/pending") return pending;
      if (repo === "acme/broken") return Promise.reject(new Error("GitHub rate limit exceeded"));
      return Promise.resolve(states[repo]);
    });
    list.mockResolvedValue([]);
    await mountWithDiscovery(
      [...Object.keys(states), "acme/pending", "acme/broken"].map(candidate),
    );

    await vi.waitFor(() =>
      expect(cardButtons("acme/install")).toEqual(["Install v1.2.0", "Open repository"]),
    );
    expect(cardButtons("acme/current")).toEqual(["Open repository"]);
    expect(card("acme/current")?.textContent).toContain("Up to date (v1.0.0)");
    expect(cardButtons("acme/update")).toEqual(["Update to v2.0.0", "Open repository"]);
    expect(cardButtons("acme/reinstall")).toEqual(["Reinstall latest", "Open repository"]);
    expect(cardButtons("acme/local")).toEqual(["Replace with v1.2.0", "Open repository"]);
    expect(card("acme/local")?.textContent).toContain("Installed locally");
    expect(cardButtons("acme/unavailable")).toEqual(["Open repository"]);
    expect(card("acme/unavailable")?.textContent).toContain(
      "Not available for macos-arm64: the latest release ships no matching asset.",
    );
    await vi.waitFor(() =>
      expect(card("acme/broken")?.textContent).toContain("GitHub rate limit exceeded"),
    );
    expect(cardButtons("acme/broken")).toEqual(["Open repository"]);
    expect(card("acme/pending")?.textContent).toContain("Checking…");
    expect(cardButtons("acme/pending")).toEqual(["Open repository"]);
    expect(target.textContent).not.toContain("rate limit exceeded.");

    settlePending({ kind: "install", latest_version: null });
    await vi.waitFor(() =>
      expect(cardButtons("acme/pending")).toEqual(["Install", "Open repository"]),
    );
  });

  it("stages a Discovery install, reviews it, and commits it", async () => {
    list.mockResolvedValue([]);
    resolveCandidate.mockResolvedValue({ kind: "install", latest_version: "1.2.0" });
    let finishStaging: (staged: StagedPluginInstall) => void = () => {};
    stageRemote.mockReturnValue(
      new Promise<StagedPluginInstall>((resolve) => {
        finishStaging = resolve;
      }),
    );
    commitStaged.mockResolvedValue(remoteAcmePlugin);
    await mountWithDiscovery([candidate("acme/planeai-plugin-acme")]);

    await vi.waitFor(() => expect(buttonLabeled(target, "Install v1.2.0")).toBeDefined());
    buttonLabeled(target, "Install v1.2.0")?.click();
    await vi.waitFor(() => expect(stageRemote).toHaveBeenCalledWith("acme/planeai-plugin-acme"));
    await vi.waitFor(() => expect(buttonLabeled(target, "Downloading…")?.disabled).toBe(true));

    finishStaging(stagedAcme);
    await vi.waitFor(() => expect(reviewDialog()?.textContent).toContain("Install Acme?"));
    const listCalls = list.mock.calls.length;
    buttonLabeled(reviewDialog()!, "Install and enable")?.click();

    await vi.waitFor(() => expect(commitStaged).toHaveBeenCalledWith("stage-1"));
    await vi.waitFor(() => expect(reviewDialog()).toBeNull());
    await vi.waitFor(() => expect(list).toHaveBeenCalledTimes(listCalls + 1));
    await vi.waitFor(() => expect(resolveCandidate).toHaveBeenCalledTimes(2));
    expect(resolveCandidate).toHaveBeenLastCalledWith("acme/planeai-plugin-acme");
    expect(discardStaged).not.toHaveBeenCalled();
  });

  it("discards the staged package when the review is cancelled", async () => {
    list.mockResolvedValue([]);
    resolveCandidate.mockResolvedValue({ kind: "install", latest_version: "1.2.0" });
    stageRemote.mockResolvedValue(stagedAcme);
    discardStaged.mockResolvedValue(undefined);
    await mountWithDiscovery([candidate("acme/planeai-plugin-acme")]);

    await vi.waitFor(() => expect(buttonLabeled(target, "Install v1.2.0")).toBeDefined());
    buttonLabeled(target, "Install v1.2.0")?.click();
    await vi.waitFor(() => expect(reviewDialog()?.textContent).toContain("Install Acme?"));
    buttonLabeled(reviewDialog()!, "Cancel")?.click();

    await vi.waitFor(() => expect(discardStaged).toHaveBeenCalledWith("stage-1"));
    await vi.waitFor(() => expect(reviewDialog()).toBeNull());
    expect(commitStaged).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(buttonLabeled(target, "Install v1.2.0")?.disabled).toBe(false));
  });

  it("reports a failed commit once the review closes and re-resolves the card", async () => {
    const repo = "acme/planeai-plugin-acme";
    list.mockResolvedValue([]);
    resolveCandidate
      .mockResolvedValueOnce({ kind: "install", latest_version: "1.2.0" })
      .mockResolvedValue({ kind: "reinstall", installed_version: "1.2.0" });
    stageRemote.mockResolvedValue(stagedAcme);
    commitStaged.mockRejectedValue(new Error("plugin backend exited during startup"));
    await mountWithDiscovery([candidate(repo)]);

    await vi.waitFor(() => expect(buttonLabeled(target, "Install v1.2.0")).toBeDefined());
    buttonLabeled(target, "Install v1.2.0")?.click();
    await vi.waitFor(() => expect(reviewDialog()?.textContent).toContain("Install Acme?"));
    buttonLabeled(reviewDialog()!, "Install and enable")?.click();

    await vi.waitFor(() => expect(reviewDialog()).toBeNull());
    await vi.waitFor(() =>
      expect(paragraphs()).toContain("Error: plugin backend exited during startup"),
    );
    await vi.waitFor(() =>
      expect(cardButtons(repo)).toEqual(["Reinstall latest", "Open repository"]),
    );
    expect(discardStaged).not.toHaveBeenCalled();
  });

  it("shows a failed download and offers the install again", async () => {
    const repo = "acme/planeai-plugin-acme";
    list.mockResolvedValue([]);
    resolveCandidate.mockResolvedValue({ kind: "install", latest_version: "1.2.0" });
    stageRemote.mockRejectedValue(
      new Error("plugin download failed with HTTP 404: https://cdn.acme.dev/acme.tar.gz"),
    );
    await mountWithDiscovery([candidate(repo)]);

    await vi.waitFor(() => expect(buttonLabeled(target, "Install v1.2.0")).toBeDefined());
    buttonLabeled(target, "Install v1.2.0")?.click();

    await vi.waitFor(() =>
      expect(paragraphs()).toContain(
        "Error: plugin download failed with HTTP 404: https://cdn.acme.dev/acme.tar.gz",
      ),
    );
    expect(reviewDialog()).toBeNull();
    expect(buttonLabeled(target, "Install v1.2.0")?.disabled).toBe(false);
  });

  it("shows the GitHub origin instead of the import path for Discovery installs", async () => {
    const untaggedPlugin = {
      ...remoteAcmePlugin,
      id: "acme-nightly",
      name: "Acme Nightly",
      remote_origin: {
        ...remoteAcmePlugin.remote_origin,
        repo: "acme/planeai-plugin-nightly",
        release_tag: null,
      },
    };
    list.mockResolvedValue([remoteAcmePlugin, untaggedPlugin, disabledGithubPlugin]);
    target = document.createElement("div");
    document.body.append(target);
    component = mount(PluginManager, { target });

    const sourceLines = () =>
      Array.from(target.querySelectorAll("p"))
        .map((line) => line.textContent?.trim() ?? "")
        .filter((text) => text.startsWith("Source: "));
    await vi.waitFor(() =>
      expect(sourceLines()).toEqual([
        "Source: github.com/acme/planeai-plugin-acme @ v1.2.0",
        "Source: github.com/acme/planeai-plugin-nightly",
        "Source: /tmp/planeai-plugin-github",
      ]),
    );
    expect(target.textContent).toContain("Installed SHA-256: acme-hash");
  });
});
