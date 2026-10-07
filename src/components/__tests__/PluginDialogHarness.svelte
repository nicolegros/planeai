<script lang="ts">
  import PluginDialog from "../PluginDialog.svelte";
  import type { PluginInventory, PluginUiContribution } from "../../lib/types";

  let { placement }: { placement: PluginUiContribution["placement"] } = $props();
  let open = $state(true);
  const plugin: PluginInventory = {
    id: "routines",
    name: "Routines",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "local",
    backend_entrypoint: "bin/planeai-plugin-routines",
    capabilities: ["settings"],
    ui_contributions: [],
    providers: [],
    installed_hash: "hash",
    installed_path: "/planeai/plugins/packages/sha256/hash",
    original_display_path: "/source/routines",
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  };
  const contribution = $derived<PluginUiContribution>({ id: "routines", label: "Routines", placement, entrypoint: "ui/routines.js", order: null, shortcut: null });
</script>

{#if open}
  <PluginDialog {plugin} {contribution} getFocusedAgentSession={() => undefined} onNavigate={() => {}} onClose={() => (open = false)} onOpenPreferences={() => {}} />
{/if}
