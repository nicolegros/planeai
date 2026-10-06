import type { PluginInventory, PluginProvider, PluginUiContribution, Session } from "./types";

/** `sessions.backend` for sessions run by a plugin provider. */
const PLUGIN_BACKEND = "plugin";

/** Whether a plugin provider runs the session, rather than a terminal. */
export function isPluginSession(session: Pick<Session, "backend">): boolean {
  return session.backend === PLUGIN_BACKEND;
}

export interface RuntimeProvider {
  /** `<plugin id>:<provider id>`, the value stored as the session's provider. */
  key: string;
  plugin: PluginInventory;
  provider: PluginProvider;
}

function runtimeProviderKey(pluginId: string, providerId: string): string {
  return `${pluginId}:${providerId}`;
}

/** Keys with `:` belong to plugin providers, never to configured ones (as the host's contract says). */
export function isReservedProviderKey(key: string): boolean {
  return key.includes(":");
}

/** The plugin and provider ids in a provider key; `null` for a configured provider's key. */
function parseProviderKey(key: string): { pluginId: string; providerId: string } | null {
  const separator = key.indexOf(":");
  if (separator <= 0 || separator === key.length - 1) return null;
  return { pluginId: key.slice(0, separator), providerId: key.slice(separator + 1) };
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
  const parsed = key ? parseProviderKey(key) : null;
  if (!parsed) return null;
  const { pluginId, providerId } = parsed;
  const plugin = inventory.find((candidate) => candidate.id === pluginId);
  const provider = plugin?.providers.find((candidate) => candidate.id === providerId);
  return plugin && provider ? { plugin, provider } : null;
}

export function supportsAutoApprove(provider: PluginProvider): boolean {
  return provider.supports.includes("auto_approve");
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
