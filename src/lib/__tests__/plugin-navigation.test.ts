import { describe, expect, it } from "vitest";
import { metaSlotPanelOf, resolvePluginOpen } from "../plugin-navigation";
import type { PluginInventory, PluginUiContribution } from "../types";

function contribution(
  id: string,
  placement: PluginUiContribution["placement"],
): PluginUiContribution {
  return { id, label: id, placement, entrypoint: `ui/${id}.js`, order: null, shortcut: null };
}

function plugin(
  id: string,
  ui_contributions: PluginUiContribution[],
  state: PluginInventory["state"] = "running",
): PluginInventory {
  return {
    id,
    name: id,
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "local",
    backend_entrypoint: "bin/plugin",
    capabilities: [],
    ui_contributions,
    providers: [],
    installed_hash: null,
    installed_path: null,
    original_display_path: null,
    remote_origin: null,
    enabled: true,
    state,
    last_error: null,
    log_path: null,
  };
}

const inventory = [
  plugin("routines", [
    contribution("routines", "dialog"),
    contribution("sidebar", "sidebar.header"),
  ]),
  plugin("github", [contribution("pull-request", "session.panel")]),
  plugin("fixture", [contribution("dashboard", "main-pane")]),
  plugin("stopped", [contribution("manager", "dialog")], "disabled"),
];

function surfaceOf(
  pluginId: string,
  contributionId: string,
  origin: Parameters<typeof resolvePluginOpen>[3],
  hasSession: boolean,
) {
  const target = resolvePluginOpen(inventory, pluginId, contributionId, origin, hasSession);
  return target.surface === "unavailable"
    ? "unavailable"
    : `${target.surface}:${target.plugin.id}/${target.contribution.id}`;
}

describe("resolvePluginOpen", () => {
  it("opens a dialog contribution in the host dialog from navigation.open, the command menu and its shortcut", () => {
    expect(surfaceOf("routines", "routines", "navigation", true)).toBe("dialog:routines/routines");
    expect(surfaceOf("routines", "routines", "command", true)).toBe("dialog:routines/routines");
    expect(surfaceOf("routines", "routines", "shortcut", true)).toBe("dialog:routines/routines");
  });

  it("opens a dialog contribution without a selected session", () => {
    expect(surfaceOf("routines", "routines", "navigation", false)).toBe("dialog:routines/routines");
    expect(surfaceOf("routines", "routines", "command", false)).toBe("dialog:routines/routines");
  });

  it("keeps a session panel in the dialog from its titlebar entry, shortcut and meta slot, and in the main pane otherwise", () => {
    expect(surfaceOf("github", "pull-request", "titlebar", true)).toBe(
      "dialog:github/pull-request",
    );
    expect(surfaceOf("github", "pull-request", "shortcut", true)).toBe(
      "dialog:github/pull-request",
    );
    expect(surfaceOf("github", "pull-request", "session-meta", true)).toBe(
      "dialog:github/pull-request",
    );
    expect(surfaceOf("github", "pull-request", "navigation", true)).toBe(
      "main-pane:github/pull-request",
    );
    expect(surfaceOf("github", "pull-request", "command", true)).toBe(
      "main-pane:github/pull-request",
    );
  });

  it("refuses a session panel without a selected session", () => {
    expect(surfaceOf("github", "pull-request", "titlebar", false)).toBe("unavailable");
    expect(surfaceOf("github", "pull-request", "navigation", false)).toBe("unavailable");
  });

  it("opens a main pane from navigation.open whatever the session", () => {
    expect(surfaceOf("fixture", "dashboard", "navigation", false)).toBe(
      "main-pane:fixture/dashboard",
    );
  });

  it("refuses placements that are not destinations, unknown targets and stopped plugins", () => {
    expect(surfaceOf("routines", "sidebar", "navigation", true)).toBe("unavailable");
    expect(surfaceOf("routines", "missing", "navigation", true)).toBe("unavailable");
    expect(surfaceOf("stopped", "manager", "navigation", true)).toBe("unavailable");
  });
});

describe("metaSlotPanelOf", () => {
  const withApi = (
    host_api_version: string,
    contributions = [contribution("pull-request", "session.panel")],
  ) => ({
    ...plugin("github", contributions),
    host_api_version,
  });

  it("gives a meta slot its plugin's first session panel from host API v5 on", () => {
    expect(metaSlotPanelOf(withApi("planeai.plugin-host.v5"))?.id).toBe("pull-request");
    expect(metaSlotPanelOf(withApi("planeai.plugin-host.v10"))?.id).toBe("pull-request");
  });

  it("keeps the slot visual-only before v5, for an unknown version, or without a session panel", () => {
    expect(metaSlotPanelOf(withApi("planeai.plugin-host.v4"))).toBeUndefined();
    expect(metaSlotPanelOf(withApi("planeai.plugin-host.vx"))).toBeUndefined();
    expect(metaSlotPanelOf(withApi("other.v9"))).toBeUndefined();
    expect(
      metaSlotPanelOf(withApi("planeai.plugin-host.v5", [contribution("d", "dialog")])),
    ).toBeUndefined();
  });
});
