<script lang="ts">
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import type { PluginSessionContext } from "../../lib/plugin-sdk";
  import type { ProviderSessionBridge } from "../../lib/provider-session-bridge";
  import type { PluginInventory, PluginUiContribution } from "../../lib/types";

  let { plugin, contribution, session, providerSession, onFocused }: { plugin: PluginInventory; contribution: PluginUiContribution; session: PluginSessionContext; providerSession: ProviderSessionBridge; onFocused: () => void } = $props();

  let focused = $state(false);

  export function setFocused(next: boolean): void {
    focused = next;
  }
</script>

<PluginContributionHost {plugin} {contribution} {session} {providerSession} autofocus={focused} {onFocused} onNavigate={() => {}} onClose={() => {}} />
