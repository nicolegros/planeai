import type { PluginInventory, PluginUiContribution } from "./types";

/** How the user asked to open a contribution. */
export type PluginOpenOrigin = "navigation" | "command" | "shortcut" | "titlebar" | "session-meta";

export type PluginOpenTarget =
  | { surface: "dialog" | "main-pane"; plugin: PluginInventory; contribution: PluginUiContribution }
  | { surface: "unavailable" };

/**
 * Where a contribution opens. A dialog opens in the host dialog from anywhere and
 * needs no session. A session panel needs the selected session: its titlebar entry,
 * shortcut and sidebar meta slot open it in the dialog, other routes show it as a main pane.
 */
export function resolvePluginOpen(
  inventory: readonly PluginInventory[],
  pluginId: string,
  contributionId: string,
  origin: PluginOpenOrigin,
  hasSession: boolean,
): PluginOpenTarget {
  const plugin = inventory.find(
    (candidate) => candidate.id === pluginId && candidate.state === "running",
  );
  const contribution = plugin?.ui_contributions.find(
    (candidate) => candidate.id === contributionId,
  );
  if (!plugin || !contribution) return { surface: "unavailable" };
  switch (contribution.placement) {
    case "dialog":
      return { surface: "dialog", plugin, contribution };
    case "main-pane":
      return { surface: "main-pane", plugin, contribution };
    case "session.panel":
      if (!hasSession) return { surface: "unavailable" };
      return {
        surface: origin === "navigation" || origin === "command" ? "main-pane" : "dialog",
        plugin,
        contribution,
      };
    default:
      return { surface: "unavailable" };
  }
}

const HOST_API_PREFIX = "planeai.plugin-host.v";
/**
 * Host API v5 (`SESSION_META_HOST_API_VERSION` in the plugin contract, which validates the version string)
 * brought the meta slot's tooltip and click-through and the in-place session panel refresh; older plugins keep
 * a visual-only slot and a remounted panel.
 */
const SESSION_UI_HOST_API = 5;

function hostApiNumber(version: string): number {
  return version.startsWith(HOST_API_PREFIX)
    ? Number(version.slice(HOST_API_PREFIX.length)) || 0
    : 0;
}

/** Whether a plugin's session panel is refreshed in place on data changes instead of remounted. */
export function refreshesSessionPanelInPlace(plugin: PluginInventory): boolean {
  return hostApiNumber(plugin.host_api_version) >= SESSION_UI_HOST_API;
}

/** The session panel a plugin's sidebar meta slot opens: its first, in contribution order. */
export function metaSlotPanelOf(plugin: PluginInventory): PluginUiContribution | undefined {
  if (hostApiNumber(plugin.host_api_version) < SESSION_UI_HOST_API) return undefined;
  return plugin.ui_contributions.find((contribution) => contribution.placement === "session.panel");
}
