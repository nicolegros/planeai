<script lang="ts">
  import { onMount } from "svelte";
  import UnifiedSidebar from "../UnifiedSidebar.svelte";
  import { registerPluginSidebarContribution } from "../../lib/plugin-sidebar-navigation.svelte";
  import type { PluginInventory } from "../../lib/types";

  let { includeLocalContribution = false }: { includeLocalContribution?: boolean } = $props();

  const precedingPlugin: PluginInventory = {
    id: "sidebar-fixture",
    name: "Sidebar Fixture",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "builtin",
    backend_entrypoint: "built-in",
    capabilities: [],
    ui_contributions: [
      {
        id: "preceding-row",
        label: "Preceding row",
        placement: "sidebar.section",
        entrypoint: "unavailable",
        order: null,
        shortcut: null,
      },
    ],
    installed_hash: null,
    installed_path: null,
    original_display_path: null,
    enabled: false,
    state: "disabled",
    last_error: null,
    log_path: null,
  };

  const failingLocalPlugin: PluginInventory = {
    id: "local-fixture",
    name: "Local Fixture",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "local",
    backend_entrypoint: "bin/planeai-plugin-fixture",
    capabilities: [],
    ui_contributions: [
      {
        id: "log",
        label: "Log",
        placement: "sidebar.section",
        entrypoint: "ui/entry.js",
        order: null,
        shortcut: null,
      },
    ],
    installed_hash: "fixture-hash",
    installed_path: "/planeai/plugins/packages/sha256/fixture-hash",
    original_display_path: "/source/local-fixture",
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  };

  const jiraPlugin: PluginInventory = {
    id: "jira",
    name: "Jira",
    version: "0.1.0",
    host_api_version: "planeai.plugin-host.v1",
    source_kind: "builtin",
    backend_entrypoint: "built-in",
    capabilities: [],
    ui_contributions: [
      {
        id: "section",
        label: "Jira",
        placement: "sidebar.section",
        entrypoint: "jira-sidebar-section",
        order: null,
        shortcut: null,
      },
    ],
    installed_hash: null,
    installed_path: null,
    original_display_path: null,
    enabled: true,
    state: "running",
    last_error: null,
    log_path: null,
  };

  onMount(() =>
    registerPluginSidebarContribution("sidebar-fixture:preceding-row", [{ id: "before" }]),
  );

  const noop = () => {};
</script>

<UnifiedSidebar
  renamingSessionId={null}
  onAddProject={noop}
  onSelectSession={noop}
  onArchiveSession={noop}
  onDeleteSession={noop}
  onRestartSession={noop}
  onOpenPreferences={noop}
  onRenameSession={noop}
  onStartRename={noop}
  onDeleteProject={noop}
  onEditProject={noop}
  onPickTask={noop}
  pluginContributions={[
    { plugin: precedingPlugin, contribution: precedingPlugin.ui_contributions[0] },
    ...(includeLocalContribution
      ? [{ plugin: failingLocalPlugin, contribution: failingLocalPlugin.ui_contributions[0] }]
      : []),
    { plugin: jiraPlugin, contribution: jiraPlugin.ui_contributions[0] },
  ]}
/>
