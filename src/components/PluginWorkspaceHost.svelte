<script lang="ts">
  import { onDestroy } from "svelte";
  import { plugins } from "../lib/api";
  import type { PluginUiDisposer, PluginUiEntrypoint } from "../lib/plugin-sdk";
  import type { PluginInventory } from "../lib/types";

  interface Props {
    plugin: PluginInventory;
  }

  type LocalPluginModule = {
    default?: PluginUiEntrypoint;
    pluginEntrypoint?: PluginUiEntrypoint;
  };

  let { plugin }: Props = $props();
  let container = $state<HTMLElement>();
  let disposer: PluginUiDisposer | null = null;
  let generation = 0;

  const bundledEntries: Record<string, () => Promise<PluginUiEntrypoint>> = {
    jira: () => import("../plugins/jira/entry").then(({ jiraStatusEntrypoint }) => jiraStatusEntrypoint),
  };

  function disposeCurrent() {
    disposer?.();
    disposer = null;
  }

  function localModuleUrl(source: string): { url: string; revoke: () => void } {
    const isJsdom = typeof navigator !== "undefined" && /jsdom/i.test(navigator.userAgent);
    if (typeof URL.createObjectURL === "function" && !isJsdom) {
      const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
      return { url, revoke: () => URL.revokeObjectURL(url) };
    }
    return {
      url: `data:text/javascript;charset=utf-8,${encodeURIComponent(source)}`,
      revoke: () => {},
    };
  }

  async function loadLocalEntrypoint(pluginId: string): Promise<PluginUiEntrypoint> {
    const source = await plugins.localUiSource(pluginId);
    const { url, revoke } = localModuleUrl(source);
    try {
      const module = await import(/* @vite-ignore */ url) as LocalPluginModule;
      const entrypoint = module.default ?? module.pluginEntrypoint;
      if (!entrypoint || typeof entrypoint.mount !== "function") {
        throw new Error("local UI bundle must default-export a PluginUiEntrypoint");
      }
      return entrypoint;
    } finally {
      revoke();
    }
  }

  async function loadEntrypoint(selected: PluginInventory): Promise<PluginUiEntrypoint> {
    if (selected.source_kind === "local") return loadLocalEntrypoint(selected.id);
    const loader = bundledEntries[selected.id];
    if (!loader) throw new Error("This builtin plugin has no registered workspace UI.");
    return loader();
  }

  async function mountPlugin(target: HTMLElement, selected: PluginInventory, version: number) {
    disposeCurrent();
    const root = target.shadowRoot ?? target.attachShadow({ mode: "open" });
    if (selected.state !== "running") {
      root.replaceChildren();
      return;
    }
    if (!selected.ui_entrypoint) {
      root.replaceChildren(document.createTextNode("This plugin has no workspace UI."));
      return;
    }
    try {
      const entrypoint = await loadEntrypoint(selected);
      if (version !== generation) return;
      const cleanup = entrypoint.mount(root, {
        plugin: selected,
        host: {
          call: <T>(method: string, params: unknown = null) =>
            plugins.call<T>(selected.id, method, params),
        },
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
