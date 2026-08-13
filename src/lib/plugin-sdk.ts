import type { PluginInventory, PluginUiContribution } from "./types";

/**
 * Plugin UI modules use only this host-injected bridge. The host scopes every
 * request to the selected plugin runtime; modules never invoke Tauri directly.
 */
export interface PluginUiHost {
  call<T>(method: string, params?: unknown): Promise<T>;
  navigation: {
    open(pluginId: string, contributionId: string): void;
    close(): void;
  };
}

export interface PluginUiContext {
  plugin: PluginInventory;
  contribution: PluginUiContribution;
  host: PluginUiHost;
}

export type PluginUiDisposer = () => void;

export interface PluginUiEntrypoint {
  mount(root: ShadowRoot, context: PluginUiContext): PluginUiDisposer;
}
