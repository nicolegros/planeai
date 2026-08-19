<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { jiraPreferencesEntrypoint, jiraSidebarSectionEntrypoint, jiraStatusEntrypoint } from "../plugins/jira/entry";
  import { plugins, projects as projectsApi, tasks as tasksApi } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import * as taskStore from "../lib/task-store.svelte";
  import { getTasksByProject } from "../lib/task-store.svelte";
  import { openPluginModal, openProjectForm } from "../lib/plugin-modal-manager";
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

  async function callPlugin<T>(method: string, params: unknown = null): Promise<T> {
    const value = await plugins.call<T>(plugin.id, method, params);
    if (plugin.id !== "jira" || method !== "jira.sidebar.items") return value;
    const sidebar = value as { items?: Array<{ key: string; child_count?: number }> };
    if (!sidebar.items) return value;
    const projectList = await projectsApi.list();
    const taskLists = await Promise.all(projectList.map((project) => tasksApi.listAll(project.path)));
    const counts = new Map<string, number>();
    for (const task of taskLists.flat()) {
      if (task.parent_key) counts.set(task.parent_key, (counts.get(task.parent_key) ?? 0) + 1);
    }
    return {
      ...sidebar,
      items: sidebar.items.map((item) => ({ ...item, child_count: counts.get(item.key) ?? 0 })),
    } as T;
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
          call: <T>(method: string, params: unknown = null) => callPlugin<T>(method, params),
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
            refreshAssignment: async (project) => {
              try {
                await Promise.all([
                  taskStore.refresh([...new Set([...Object.keys(getTasksByProject()), project.path])]),
                  plugins.dataChanged(plugin.id),
                ]);
              } catch (error) {
                showSnackbar(`Child task created, but refresh failed: ${String(error)}`);
              }
            },
            notify: (message, kind = "error") => showSnackbar(message, kind),
          },
          projects: {
            list: async () => (await projectsApi.list()).filter((project) => !project.hidden),
          },
          tasks: {
            createChild: ({ project, title, description, parentKey }) =>
              tasksApi.create({
                repoPath: project.path,
                title,
                description,
                priority: 0,
                tags: [],
                blockedBy: [],
                parentKey,
              }),
          },
          interaction: {
            openModal: openPluginModal,
            openProjectForm,
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
