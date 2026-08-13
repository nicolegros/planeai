<script lang="ts">
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import type { PluginInventory } from "../../lib/types";

  const plugin: PluginInventory = {
    id: "local-fixture",
    name: "Local Fixture",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "local",
    backend_entrypoint: "bin/planeai-plugin-fixture",
    ui_contributions: [{ id: "fixture", label: "Fixture", placement: "main-pane", entrypoint: "ui/entry.js", order: null, shortcut: null }],
    installed_hash: "fixture-hash",
    installed_path: "/planeai/plugins/packages/sha256/fixture-hash",
    original_display_path: "/source/local-fixture",
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  };
</script>

<PluginContributionHost {plugin} contribution={plugin.ui_contributions[0]} onNavigate={() => {}} onClose={() => {}} />
