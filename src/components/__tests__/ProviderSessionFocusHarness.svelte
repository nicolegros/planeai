<script lang="ts">
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import type { PluginSessionContext } from "../../lib/plugin-sdk";
  import type { PluginInventory, PluginUiContribution } from "../../lib/types";

  let { plugin, contribution, session, onFocused }: { plugin: PluginInventory; contribution: PluginUiContribution; session: PluginSessionContext; onFocused: () => void } = $props();

  let focused = $state(false);

  export function setFocused(next: boolean): void {
    focused = next;
  }
</script>

<PluginContributionHost {plugin} {contribution} {session} autofocus={focused} {onFocused} onNavigate={() => {}} onClose={() => {}} />
