<script lang="ts">
  import PluginWorkspaceHost from "../PluginWorkspaceHost.svelte";
  import type { PluginInventory } from "../../lib/types";

  const plugin: PluginInventory = {
    id: "local-fixture",
    name: "Local Fixture",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "local",
    backend_entrypoint: "bin/planeai-plugin-fixture",
    ui_entrypoint: "ui/entry.js",
    installed_hash: "fixture-hash",
    installed_path: "/planeai/plugins/packages/sha256/fixture-hash",
    original_display_path: "/source/local-fixture",
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  };
</script>

<PluginWorkspaceHost {plugin} />
