<script lang="ts">
  import { onDestroy } from "svelte";
  import { plugins } from "../lib/api";
  import type { PluginUiDisposer, PluginUiEntrypoint } from "../lib/plugin-sdk";
  import type { PluginInventory } from "../lib/types";

  interface Props {
    plugin: PluginInventory;
  }

  let { plugin }: Props = $props();
  let container = $state<HTMLElement>();
  let disposer: PluginUiDisposer | null = null;
  let generation = 0;

  const entries: Record<string, () => Promise<PluginUiEntrypoint>> = {
    jira: () => import("../plugins/jira/entry").then(({ jiraStatusEntrypoint }) => jiraStatusEntrypoint),
  };

  function disposeCurrent() {
    disposer?.();
    disposer = null;
  }

  async function mountPlugin(target: HTMLElement, selected: PluginInventory, version: number) {
    disposeCurrent();
    const root = target.shadowRoot ?? target.attachShadow({ mode: "open" });
    if (selected.state !== "running") {
      root.replaceChildren();
      return;
    }
    const loader = entries[selected.id];
    if (!loader || !selected.ui_entrypoint) {
      root.replaceChildren(document.createTextNode("This plugin has no workspace UI."));
      return;
    }
    try {
      const entrypoint = await loader();
      if (version !== generation) return;
      const cleanup = entrypoint.mount(root, {
        plugin: selected,
        host: { getJiraStatus: plugins.jiraStatus },
      });
      if (version !== generation) {
        cleanup();
        return;
      }
      disposer = cleanup;
    } catch (error) {
      if (version === generation) {
        root.replaceChildren(document.createTextNode(`Failed to load plugin UI: ${String(error)}`));
      }
    }
  }

  $effect(() => {
    if (!container) return;
    const version = ++generation;
    void mountPlugin(container, plugin, version);
    return () => {
      if (generation === version) generation += 1;
      disposeCurrent();
    };
  });

  onDestroy(() => {
    generation += 1;
    disposeCurrent();
  });
</script>

<div class="h-full w-full" data-plugin-workspace-host bind:this={container}></div>
