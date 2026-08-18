<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { jiraPreferencesEntrypoint, jiraSidebarSectionEntrypoint, jiraStatusEntrypoint } from "../plugins/jira/entry";
  import { plugins } from "../lib/api";
  import type { PluginUiDisposer, PluginUiEntrypoint } from "../lib/plugin-sdk";
  import { registerPluginSidebarContribution } from "../lib/plugin-sidebar-navigation.svelte";
  import type { PluginInventory, PluginUiContribution } from "../lib/types";

  interface Props {
    plugin: PluginInventory;
    contribution: PluginUiContribution;
    onNavigate: (pluginId: string, contributionId: string) => void;
    onClose: () => void;
    onOpenPreferences?: () => void;
    autofocus?: boolean;
  }
  type LocalPluginModule = { default?: PluginUiEntrypoint; pluginEntrypoint?: PluginUiEntrypoint };

  let { plugin, contribution, onNavigate, onClose, onOpenPreferences = () => {}, autofocus = false }: Props = $props();
  let container = $state<HTMLElement>();
  let disposer: PluginUiDisposer | null = null;
  let generation = 0;

  const bundledEntries: Record<string, () => Promise<PluginUiEntrypoint>> = {
    "jira:jira-status": async () => jiraStatusEntrypoint,
    "jira:jira-preferences": async () => jiraPreferencesEntrypoint,
    "jira:jira-sidebar-section": async () => jiraSidebarSectionEntrypoint,
  };

  function disposeCurrent(): void {
    disposer?.();
    disposer = null;
  }

  function localModuleUrl(source: string): { url: string; revoke: () => void } {
    const jsdom = typeof navigator !== "undefined" && /jsdom/i.test(navigator.userAgent);
    if (typeof URL.createObjectURL === "function" && !jsdom) {
      const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
      return { url, revoke: () => URL.revokeObjectURL(url) };
    }
    return { url: `data:text/javascript;charset=utf-8,${encodeURIComponent(source)}`, revoke: () => {} };
  }

  async function loadEntrypoint(): Promise<PluginUiEntrypoint> {
    if (plugin.source_kind === "builtin") {
      const loader = bundledEntries[`${plugin.id}:${contribution.entrypoint}`];
      if (!loader) throw new Error("This bundled contribution has no trusted UI entrypoint.");
      return loader();
    }
    const source = await plugins.localUiSource(plugin.id, contribution.id);
    const { url, revoke } = localModuleUrl(source);
    try {
      const module = (await import(/* @vite-ignore */ url)) as LocalPluginModule;
      const entrypoint = module.default ?? module.pluginEntrypoint;
      if (!entrypoint || typeof entrypoint.mount !== "function") {
        throw new Error("local UI bundle must default-export a PluginUiEntrypoint");
      }
      return entrypoint;
    } finally {
      revoke();
    }
  }

  function retry(): void {
    if (!container) return;
    const version = ++generation;
    void mountContribution(container, version);
  }

  function showLoadFailure(root: ShadowRoot, error: unknown): void {
    const message = document.createElement("p");
    message.setAttribute("role", "alert");
    message.textContent = `Failed to load ${contribution.label}: ${String(error)}`;
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Retry";
    button.addEventListener("click", retry);
    root.replaceChildren(message, button);
    button.focus();
  }

  async function mountContribution(target: HTMLElement, version: number): Promise<void> {
    disposeCurrent();
    const root = target.shadowRoot ?? target.attachShadow({ mode: "open" });
    if (plugin.state !== "running") {
      root.replaceChildren();
      return;
    }
    try {
      const entrypoint = await loadEntrypoint();
      if (version !== generation) return;
      const cleanup = entrypoint.mount(root, {
        plugin,
        contribution,
        host: {
          call: <T>(method: string, params: unknown = null) => plugins.call<T>(plugin.id, method, params),
          navigation: { open: onNavigate, close: onClose, openPreferences: onOpenPreferences },
          sidebar: {
            register: (rows) => registerPluginSidebarContribution(`${plugin.id}:${contribution.id}`, rows),
            select: (rowId) => {
              container?.dispatchEvent(
                new CustomEvent("plugin-sidebar-select", { bubbles: true, detail: { rowId } }),
              );
            },
            handleKeydown: (event) => {
              const detail: { event: KeyboardEvent; handled: boolean } = { event, handled: false };
              container?.dispatchEvent(
                new CustomEvent("plugin-sidebar-keydown", { bubbles: true, detail }),
              );
              if (detail.handled) event.stopPropagation();
            },
          },
          data: {
            changed: () => plugins.dataChanged(plugin.id),
          },
        },
      });
      if (version !== generation) {
        cleanup();
        return;
      }
      disposer = cleanup;
      if (autofocus) target.focus();
    } catch (error) {
      if (version === generation) showLoadFailure(root, error);
    }
  }

  $effect(() => {
    if (!container) return;
    const version = ++generation;
    void mountContribution(container, version);
    return () => {
      if (generation === version) generation += 1;
      disposeCurrent();
    };
  });

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<string>("plugin-data-changed", (event) => {
      if (event.payload === plugin.id && contribution.placement === "sidebar.section") retry();
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  onDestroy(() => {
    generation += 1;
    disposeCurrent();
  });
</script>

<div
  class="h-full w-full"
  tabindex="-1"
  role="region"
  aria-label={`${plugin.name} · ${contribution.label}`}
  data-plugin-ui-contribution={`${plugin.id}:${contribution.id}`}
  data-plugin-sidebar-contribution={contribution.placement === "sidebar.section" ? "" : undefined}
  bind:this={container}
></div>
