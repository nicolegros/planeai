import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Titlebar from "../Titlebar.svelte";
import type { PluginInventory, PluginUiContribution } from "../../lib/types";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

const githubPlugin: PluginInventory = {
  id: "github",
  name: "GitHub",
  version: "0.1.0",
  host_api_version: "planeai.plugin-host.v1",
  source_kind: "local",
  backend_entrypoint: "bin/macos-arm64/planeai-plugin-github",
  capabilities: ["settings", "sessions.read", "sessions.repository-context"],
  ui_contributions: [],
  installed_hash: "plugin-hash",
  installed_path: "/planeai/plugins/github",
  original_display_path: "/Developer/planeai-plugin-github",
  enabled: true,
  state: "running",
  last_error: null,
  log_path: null,
};

const githubTitlebar: PluginUiContribution = {
  id: "pull-request-titlebar",
  label: "GitHub",
  placement: "titlebar",
  entrypoint: "ui/titlebar.js",
  order: null,
  shortcut: null,
};

const baseProps = {
  projectName: "PlaneAI",
  sessionName: "GitHub migration",
  sidebarVisible: true,
  tabs: [{ index: 0, label: "Agent", icon: "bot" }],
  activeTabIndex: 0,
  prState: "open",
  ciStatus: null,
  hasChanges: true,
  sessionId: "session-42",
  symphonyStatus: null,
  runningCount: 1,
  activeProvider: "kiro",
  onSelectTab: vi.fn(),
  onCloseTab: vi.fn(),
  onAddTab: vi.fn(),
};

describe("Titlebar", () => {
  let target: HTMLElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(() => {
    if (component) unmount(component);
    target?.remove();
  });

  it("renders a plugin-owned titlebar control beside the unchanged legacy PR chip", () => {
    const onTogglePrPanel = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(Titlebar, {
      target,
      props: {
        ...baseProps,
        prUrl: "https://github.com/nicolegros/planeai/pull/42",
        titlebarContributions: [{ plugin: githubPlugin, contribution: githubTitlebar }],
        onTogglePrPanel,
      },
    });

    expect(target.querySelector('[data-plugin-titlebar-entry="github:pull-request-titlebar"]')).not.toBeNull();
    const legacy = target.querySelector<HTMLButtonElement>("[data-legacy-pr-control]");
    expect(legacy?.textContent).toContain("PR");
    legacy?.click();
    expect(onTogglePrPanel).toHaveBeenCalledOnce();
  });

  it("keeps the legacy Create PR affordance wired to its legacy callback", () => {
    const onCreatePr = vi.fn();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(Titlebar, {
      target,
      props: { ...baseProps, prUrl: null, onCreatePr },
    });

    const create = target.querySelector<HTMLButtonElement>("[data-legacy-create-pr-control]");
    expect(create?.textContent).toContain("Create PR");
    create?.click();
    expect(onCreatePr).toHaveBeenCalledOnce();
  });
});
