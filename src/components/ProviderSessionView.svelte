<script lang="ts">
  import PluginContributionHost from "./PluginContributionHost.svelte";
  import { Button } from "./ui";
  import { findRuntimeProvider, providerContribution, supportsHandoff } from "../lib/plugin-providers";
  import type { PluginSessionContext } from "../lib/plugin-sdk";
  import { createProviderSessionBridge } from "../lib/provider-session-bridge";
  import { pluginPreferencesLocation, type SettingsLocation } from "../lib/settings-registry";
  import type { PluginInventory, Session } from "../lib/types";

  interface Props {
    session: Session;
    inventory: PluginInventory[];
    /** The chat owns the keyboard, as a terminal pane's `focused`; it takes focus whenever this turns true. */
    focused?: boolean;
    /** The user focused the chat, e.g. by clicking into it. */
    onFocused?: () => void;
    onNavigate: (pluginId: string, contributionId: string) => void;
    onOpenPreferences: (location: SettingsLocation) => void;
    onHandoff: (sessionId: string) => Promise<void>;
    onHandback: (sessionId: string) => Promise<void>;
  }

  let { session, inventory, focused = false, onFocused, onNavigate, onOpenPreferences, onHandoff, onHandback }: Props = $props();

  const PLUGINS_PAGE: SettingsLocation = { kind: "category", id: "plugins" };

  // Any plugin's change replaces the whole inventory. Comparing this provider's entry by value
  // keeps the chat mounted unless its own plugin changed; remounting reloads it and loses its state.
  const resolvedJson = $derived(JSON.stringify(findRuntimeProvider(inventory, session.provider)));
  const resolved = $derived(JSON.parse(resolvedJson) as ReturnType<typeof findRuntimeProvider>);
  const canHandoff = $derived(resolved ? supportsHandoff(resolved.provider) : false);
  const contribution = $derived(resolved ? providerContribution(resolved.provider) : null);
  const bridge = $derived(
    resolved
      ? createProviderSessionBridge({
          pluginId: resolved.plugin.id,
          providerId: resolved.provider.id,
          sessionId: session.id,
          handoff: canHandoff ? () => onHandoff(session.id) : undefined,
          handback: canHandoff ? () => onHandback(session.id) : undefined,
        })
      : undefined,
  );
  const sessionContext = $derived<PluginSessionContext>({
    id: session.id,
    projectId: session.project_id,
    branch: session.branch,
    baseBranch: session.base_branch,
    status: session.status,
    provider: session.provider,
    taskKey: session.task_key,
  });
</script>

<div class="h-full w-full bg-main">
  {#if session.status === "exited"}
    <div class="flex h-full flex-col items-center justify-center gap-3 px-6 text-center" role="status">
      <p class="text-sm text-t1">This session has exited.</p>
      <p class="max-w-lg text-xs text-t3">Restart it to continue the conversation.</p>
    </div>
  {:else if resolved && contribution && resolved.plugin.state === "running"}
    <PluginContributionHost
      plugin={resolved.plugin}
      {contribution}
      session={sessionContext}
      autofocus={focused}
      {onFocused}
      {onNavigate}
      onClose={() => {}}
      onOpenPreferences={() => onOpenPreferences(pluginPreferencesLocation(resolved.plugin) ?? PLUGINS_PAGE)}
      providerSession={bridge}
    />
  {:else}
    <div class="flex h-full flex-col items-center justify-center gap-3 px-6 text-center" role="status">
      <p class="text-sm text-t1">
        {#if resolved}
          {resolved.provider.label} is unavailable because {resolved.plugin.name} is {resolved.plugin.state === "error" ? "failing" : resolved.plugin.state}.
        {:else}
          The plugin that provides {session.provider ?? "this session"} is not installed.
        {/if}
      </p>
      {#if resolved?.plugin.last_error}
        <p class="max-w-lg text-xs text-t3">{resolved.plugin.last_error}</p>
      {/if}
      <Button onclick={() => onOpenPreferences(PLUGINS_PAGE)}>Open Plugins</Button>
    </div>
  {/if}
</div>
