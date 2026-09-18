import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Titlebar from "../Titlebar.svelte";
import type { PluginInventory, PluginUiContribution } from "../../lib/types";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
const plugin: PluginInventory = {
  id: "github",
  name: "GitHub",
  version: "0.1.0",
  host_api_version: "planeai.plugin-host.v1",
  source_kind: "local",
  backend_entrypoint: "bin/plugin",
  capabilities: ["sessions.repository-context"],
  ui_contributions: [],
  installed_hash: "hash",
  installed_path: "/plugin",
  original_display_path: "/plugin",
  enabled: true,
  state: "running",
  last_error: null,
  log_path: null,
};
const contribution: PluginUiContribution = {
  id: "pull-request-titlebar",
  label: "GitHub",
  placement: "titlebar",
  entrypoint: "ui/titlebar.js",
  order: null,
  shortcut: null,
};
const baseProps = {
  projectName: "PlaneAI",
  sessionName: "Migration",
  sidebarVisible: true,
  tabs: [{ index: 0, label: "Agent", icon: "bot" }],
  activeTabIndex: 0,
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
  it("keeps plugin titlebar controls while omitting legacy PR controls", () => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(Titlebar, {
      target,
      props: { ...baseProps, titlebarContributions: [{ plugin, contribution }] },
    });
    expect(
      target.querySelector('[data-plugin-titlebar-entry="github:pull-request-titlebar"]'),
    ).not.toBeNull();
    expect(target.querySelector("[data-legacy-pr-control]")).toBeNull();
    expect(target.querySelector("[data-legacy-create-pr-control]")).toBeNull();
  });
});
