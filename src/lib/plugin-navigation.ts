import { includesHostApi, SESSION_UI_HOST_API } from "./plugin-host-api";
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

/** Whether a plugin's session panel is refreshed in place on data changes instead of remounted. */
export function refreshesSessionPanelInPlace(plugin: PluginInventory): boolean {
  return includesHostApi(plugin, SESSION_UI_HOST_API);
}

/** The session panel a plugin's sidebar meta slot opens: its first, in contribution order. */
export function metaSlotPanelOf(plugin: PluginInventory): PluginUiContribution | undefined {
  if (!includesHostApi(plugin, SESSION_UI_HOST_API)) return undefined;
  return plugin.ui_contributions.find((contribution) => contribution.placement === "session.panel");
}
