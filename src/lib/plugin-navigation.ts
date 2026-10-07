import type { PluginInventory, PluginUiContribution } from "./types";

/** How the user asked to open a contribution. */
export type PluginOpenOrigin = "navigation" | "command" | "shortcut" | "titlebar";

export type PluginOpenTarget =
  | { surface: "dialog" | "main-pane"; plugin: PluginInventory; contribution: PluginUiContribution }
  | { surface: "unavailable" };

/**
 * Where a contribution opens. A dialog opens in the host dialog from anywhere and
 * needs no session. A session panel needs the selected session: its titlebar entry
 * and shortcut open it in the dialog, other routes show it as a main pane.
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
        surface: origin === "titlebar" || origin === "shortcut" ? "dialog" : "main-pane",
        plugin,
        contribution,
      };
    default:
      return { surface: "unavailable" };
  }
}
