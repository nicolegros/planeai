import type { JiraPluginStatus, PluginInventory } from "./types";

/**
 * The only host capability exposed to bundled plugin UI modules in v1.
 * Plugin modules receive this object from the host; they do not import stores
 * or invoke Tauri commands directly.
 */
export interface PluginUiHost {
  getJiraStatus(pluginId: string): Promise<JiraPluginStatus>;
}

export interface PluginUiContext {
  plugin: PluginInventory;
  host: PluginUiHost;
}

export type PluginUiDisposer = () => void;

export interface PluginUiEntrypoint {
  mount(root: ShadowRoot, context: PluginUiContext): PluginUiDisposer;
}
