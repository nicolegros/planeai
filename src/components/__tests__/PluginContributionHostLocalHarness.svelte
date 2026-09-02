<script lang="ts">
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import type { PluginInventory, PluginUiContribution } from "../../lib/types";

  interface Props {
    placement?: PluginUiContribution["placement"];
  }

  let { placement = "sidebar.section" }: Props = $props();
  let plugin = $state<PluginInventory>({
    id: "local-fixture",
    name: "Local Fixture",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "local",
    backend_entrypoint: "bin/planeai-plugin-fixture",
    capabilities: ["settings", "tasks.read", "task-events"],
    ui_contributions: [],
    installed_hash: "fixture-hash",
    installed_path: "/planeai/plugins/packages/sha256/fixture-hash",
    original_display_path: "/source/local-fixture",
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  });
  const contribution = $derived<PluginUiContribution>({
    id: "fixture",
    label: "Fixture",
    placement,
    entrypoint: "ui/entry.js",
    order: null,
    shortcut: null,
  });
</script>

<PluginContributionHost {plugin} {contribution} onNavigate={() => {}} onClose={() => {}} />
