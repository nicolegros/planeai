<script lang="ts">
  import PluginContributionHost from "./PluginContributionHost.svelte";
  import { Button } from "./ui";
  import { findRuntimeProvider, providerContribution } from "../lib/plugin-providers";
  import type { PluginSessionContext } from "../lib/plugin-sdk";
  import type { PluginInventory, Session } from "../lib/types";

  interface Props {
    session: Session;
    inventory: PluginInventory[];
    autofocus?: boolean;
    onNavigate: (pluginId: string, contributionId: string) => void;
    onOpenPreferences: () => void;
  }

  let { session, inventory, autofocus = false, onNavigate, onOpenPreferences }: Props = $props();

  const resolved = $derived(findRuntimeProvider(inventory, session.provider));
  const contribution = $derived(resolved ? providerContribution(resolved.provider) : null);
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

<div class="h-full w-full bg-main" data-provider-session={session.id}>
  {#if resolved && contribution && resolved.plugin.state === "running"}
    <PluginContributionHost
      plugin={resolved.plugin}
      {contribution}
      session={sessionContext}
      {autofocus}
      {onNavigate}
      onClose={() => {}}
      {onOpenPreferences}
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
      <Button onclick={onOpenPreferences}>Open Plugins</Button>
    </div>
  {/if}
</div>
