<script lang="ts">
  import PluginWorkspaceHost from "../PluginWorkspaceHost.svelte";
  import type { PluginInventory } from "../../lib/types";

  let plugin = $state<PluginInventory>({
    id: "jira",
    name: "Jira",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "builtin",
    backend_entrypoint: "planeai-plugin-jira",
    ui_entrypoint: "jira-status",
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  });

  export function reload(name = "Jira Reloaded") {
    plugin = { ...plugin, name };
  }

  export function setState(state: PluginInventory["state"]) {
    plugin = { ...plugin, state };
  }
</script>

<PluginWorkspaceHost {plugin} />
