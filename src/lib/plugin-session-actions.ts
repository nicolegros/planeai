import type { PluginSessionAction } from "./types";

/** Returns actions whose optional provider allowlist admits the selected session. */
export function pluginSessionActionsForProvider(
  actions: PluginSessionAction[],
  provider: string | null,
): PluginSessionAction[] {
  return actions.filter((action) =>
    action.providers.length === 0 || (provider !== null && action.providers.includes(provider)),
  );
}
