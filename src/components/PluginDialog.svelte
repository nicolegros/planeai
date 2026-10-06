<script lang="ts">
  import FormDialog from "./ui/FormDialog.svelte";
  import PluginContributionHost from "./PluginContributionHost.svelte";
  import type { PluginSessionContext } from "../lib/plugin-sdk";
  import type { PluginInventory, PluginUiContribution } from "../lib/types";

  interface Props {
    plugin: PluginInventory;
    contribution: PluginUiContribution;
    /** The selected session; only a session panel receives it. */
    session?: PluginSessionContext;
    getFocusedAgentSession: () => PluginSessionContext | undefined;
    onNavigate: (pluginId: string, contributionId: string) => void;
    onClose: () => void;
    onOpenPreferences: () => void;
  }

  let { plugin, contribution, session, getFocusedAgentSession, onNavigate, onClose, onOpenPreferences }: Props = $props();

  const sessionPanel = $derived(contribution.placement === "session.panel");
</script>

<FormDialog
  title={contribution.label}
  size={sessionPanel ? "form" : "wide"}
  class={sessionPanel ? "min-h-[min(360px,85vh)]" : ""}
  closeButton={!sessionPanel}
  preventEscapeClose={false}
  preventOpenAutoFocus={true}
  {onClose}
>
  <div>
    <PluginContributionHost
      {plugin}
      {contribution}
      session={sessionPanel ? session : undefined}
      {getFocusedAgentSession}
      {onNavigate}
      {onClose}
      {onOpenPreferences}
      closeOnEscape={true}
      autofocus={true}
    />
  </div>
</FormDialog>
