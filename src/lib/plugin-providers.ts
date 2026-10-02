import type { PluginInventory, PluginProvider, PluginUiContribution } from "./types";

/** `sessions.backend` for sessions run by a plugin provider. */
export const PROVIDER_BACKEND = "plugin";

export interface RuntimeProvider {
  /** `<plugin id>:<provider id>`, the value stored as the session's provider. */
  key: string;
  plugin: PluginInventory;
  provider: PluginProvider;
}

export function runtimeProviderKey(pluginId: string, providerId: string): string {
  return `${pluginId}:${providerId}`;
}

/** Providers offered by running plugins that hold the providers capability. */
export function runtimeProviders(inventory: PluginInventory[]): RuntimeProvider[] {
  return inventory
    .filter((plugin) => plugin.state === "running" && plugin.capabilities.includes("providers"))
    .flatMap((plugin) =>
      plugin.providers.map((provider) => ({
        key: runtimeProviderKey(plugin.id, provider.id),
        plugin,
        provider,
      })),
    );
}

/** Resolve a session's provider key against every installed plugin, running or not. */
export function findRuntimeProvider(
  inventory: PluginInventory[],
  key: string | null,
): { plugin: PluginInventory; provider: PluginProvider } | null {
  if (!key) return null;
  const separator = key.indexOf(":");
  if (separator <= 0) return null;
  const pluginId = key.slice(0, separator);
  const providerId = key.slice(separator + 1);
  const plugin = inventory.find((candidate) => candidate.id === pluginId);
  const provider = plugin?.providers.find((candidate) => candidate.id === providerId);
  return plugin && provider ? { plugin, provider } : null;
}

export function supportsYolo(provider: PluginProvider): boolean {
  return provider.supports.includes("yolo");
}

export function supportsHandoff(provider: PluginProvider): boolean {
  return provider.supports.includes("handoff");
}

/** The contribution shape PluginContributionHost mounts for a provider's session UI. */
export function providerContribution(provider: PluginProvider): PluginUiContribution {
  return {
    id: provider.id,
    label: provider.label,
    placement: "session.main",
    entrypoint: provider.entrypoint,
    order: null,
    shortcut: null,
  };
}
