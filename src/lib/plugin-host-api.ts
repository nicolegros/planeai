import type { PluginInventory } from "./types";

const HOST_API_PREFIX = "planeai.plugin-host.v";

/**
 * Host API v5 (`SESSION_META_HOST_API_VERSION` in the plugin contract, which validates the version string)
 * brought the meta slot's tooltip and click-through and the in-place session panel refresh; older plugins keep
 * a visual-only slot and a remounted panel.
 */
export const SESSION_UI_HOST_API = 5;

/** Host API v7 (`SYNC_HOST_API_VERSION`) brought `tasks.sync` and a local frame's `interaction` bridge. */
export const SYNC_HOST_API = 7;
export const SYNC_HOST_API_VERSION = `${HOST_API_PREFIX}${SYNC_HOST_API}`;

export function includesHostApi(plugin: PluginInventory, required: number): boolean {
  const version = plugin.host_api_version;
  const declared = version.startsWith(HOST_API_PREFIX)
    ? Number(version.slice(HOST_API_PREFIX.length)) || 0
    : 0;
  return declared >= required;
}

/** Whether a plugin writes PlaneAI tasks by key, so its UI follows task data changes. */
export function mirrorsTasks(plugin: PluginInventory): boolean {
  // `tasks.update` is the bundled Jira plugin's builtin-only grant.
  return plugin.capabilities.includes("tasks.sync") || plugin.capabilities.includes("tasks.update");
}
