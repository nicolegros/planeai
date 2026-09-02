<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { jiraDepartedInteractionEntrypoint, jiraPreferencesEntrypoint, jiraSidebarSectionEntrypoint, jiraStatusEntrypoint } from "../plugins/jira/entry";
  import { plugins, projects as projectsApi, tasks as tasksApi } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import * as taskStore from "../lib/task-store.svelte";
  import { getAllTasks } from "../lib/task-store.svelte";
  import { openPluginModal, openProjectForm } from "../lib/plugin-modal-manager";
  import type { PluginUiDisposer, PluginUiEntrypoint, PluginUiHost } from "../lib/plugin-sdk";
  import { registerPluginSidebarContribution } from "../lib/plugin-sidebar-navigation.svelte";
  import { focusSidebar } from "../lib/focus.svelte";
  import type { PluginInventory, PluginUiContribution } from "../lib/types";

  interface Props {
    plugin: PluginInventory;
    contribution: PluginUiContribution;
    onNavigate: (pluginId: string, contributionId: string) => void;
    onClose: () => void;
    onOpenPreferences?: () => void;
    onFailure?: (error: unknown) => void;
    autofocus?: boolean;
  }

  type LocalPluginFrameMessage = {
    type: string;
    requestId?: number;
    method?: string;
    params?: unknown;
    action?: string;
    pluginId?: string;
    contributionId?: string;
    registrationId?: string;
    rows?: Array<{ id?: unknown }>;
    rowId?: string;
    key?: string;
    altKey?: boolean;
    ctrlKey?: boolean;
    metaKey?: boolean;
    shiftKey?: boolean;
    kind?: "success" | "error";
    message?: string;
  };

  let { plugin, contribution, onNavigate, onClose, onOpenPreferences = () => {}, onFailure = () => {}, autofocus = false }: Props = $props();
  let container = $state<HTMLElement>();
  let disposer: PluginUiDisposer | null = null;
  let generation = 0;
  const dataChangeListeners = new Set<() => void>();
  const taskDataChangeListeners = new Set<() => void>();

  function subscribe(listeners: Set<() => void>, listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
  }

  function notify(listeners: Set<() => void>): void {
    for (const listener of listeners) listener();
  }

  const bundledEntries: Record<string, () => Promise<PluginUiEntrypoint>> = {
    "jira:jira-status": async () => jiraStatusEntrypoint,
    "jira:jira-preferences": async () => jiraPreferencesEntrypoint,
    "jira:jira-sidebar-section": async () => jiraSidebarSectionEntrypoint,
    "jira:jira-departed-interaction": async () => jiraDepartedInteractionEntrypoint,
  };

  function disposeCurrent(): void {
    const current = disposer;
    disposer = null;
    try {
      current?.();
    } catch (error) {
      console.error("Plugin UI disposer failed", error);
    }
  }

  function getJiraChildCounts(): Map<string, number> {
    const counts = new Map<string, number>();
    for (const task of getAllTasks()) {
      if (task.parent_key) counts.set(task.parent_key, (counts.get(task.parent_key) ?? 0) + 1);
    }
    return counts;
  }

  async function callPlugin<T>(method: string, params: unknown = null): Promise<T> {
    const value = await plugins.call<T>(plugin.id, method, params);
    if (plugin.id !== "jira" || method !== "jira.sidebar.items") return value;
    const sidebar = value as { items?: Array<{ key: string; child_count?: number }> };
    if (!sidebar.items) return value;
    const counts = getJiraChildCounts();
    return {
      ...sidebar,
      items: sidebar.items.map((item) => ({ ...item, child_count: counts.get(item.key) ?? 0 })),
    } as T;
  }

  async function loadBundledEntrypoint(): Promise<PluginUiEntrypoint> {
    const loader = bundledEntries[`${plugin.id}:${contribution.entrypoint}`];
    if (!loader) throw new Error("This bundled contribution has no trusted UI entrypoint.");
    return loader();
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
    if (contribution.placement.startsWith("sidebar.")) onFailure(error);
  }

  function createLocalPluginFrame(root: ShadowRoot): PluginUiDisposer {
    const frame = document.createElement("iframe");
    frame.title = contribution.label;
    frame.setAttribute("sandbox", "allow-scripts");
    frame.className =
      contribution.placement === "interaction" || contribution.placement === "main-pane"
        ? "block h-full w-full border-0"
        : "block w-full border-0";
    if (contribution.placement.startsWith("sidebar.")) {
      frame.style.height = contribution.placement === "sidebar.footer" ? "34px" : "160px";
      frame.addEventListener("focus", focusSidebar);
      frame.addEventListener("pointerdown", focusSidebar);
    }
    frame.srcdoc = `<!doctype html>
      <meta charset="utf-8">
      <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' blob:; style-src 'unsafe-inline'">
      <style>html,body{margin:0;min-height:100%;background:transparent}</style>
      <script>
        let cleanup = null;
        let nextRequestId = 0;
        const pending = new Map();
        const registrations = new Map();
        const send = (message) => parent.postMessage(message, "*");
        const request = (type, payload = {}) => new Promise((resolve, reject) => {
          const requestId = ++nextRequestId;
          pending.set(requestId, { resolve, reject });
          send({ type, requestId, ...payload });
        });
        let sidebarKeydownRoutingEnabled = false;
        const sidebarNavigationKeys = new Set([
          "ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "j", "k", "h", "l", "a", "r", "E", "e", "o", "R", "d", "s",
        ]);
        const isEditableTarget = (target) =>
          target instanceof Element && target.closest("input, textarea, select, [contenteditable='true']");
        const sendSidebarKeydown = (event) => {
          send({
            type: "sidebar-keydown",
            key: event.key,
            altKey: event.altKey,
            ctrlKey: event.ctrlKey,
            metaKey: event.metaKey,
            shiftKey: event.shiftKey,
          });
        };
        const forwardSidebarKeydown = (event) => {
          if (
            !sidebarKeydownRoutingEnabled ||
            !sidebarNavigationKeys.has(event.key) ||
            event.altKey ||
            event.ctrlKey ||
            event.metaKey ||
            isEditableTarget(event.target)
          ) return;
          sendSidebarKeydown(event);
          event.preventDefault();
          event.stopPropagation();
        };
        addEventListener("keydown", forwardSidebarKeydown);
        const host = {
          call: (method, params = null) => request("call", { method, params }),
          settings: {
            get: () => request("settings-get"),
            replace: (settings) => request("settings-replace", { params: settings }),
          },
          navigation: {
            open: (pluginId, contributionId) => send({ type: "navigation", action: "open", pluginId, contributionId }),
            close: () => send({ type: "navigation", action: "close" }),
            openPreferences: () => send({ type: "navigation", action: "preferences" }),
          },
          sidebar: {
            register: (rows) => {
              const registrationId = "registration:" + ++nextRequestId;
              registrations.set(registrationId, rows);
              send({ type: "sidebar-register", registrationId, rows: rows.map(({ id }) => ({ id })) });
              return () => {
                registrations.delete(registrationId);
                send({ type: "sidebar-unregister", registrationId });
              };
            },
            select: (rowId) => send({ type: "sidebar-select", rowId }),
            handleKeydown: (event) => {
              if (isEditableTarget(event.target)) return;
              sendSidebarKeydown(event);
              event.preventDefault();
              event.stopPropagation();
            },
          },
          data: {
            changed: () => request("data-changed"),
            notify: (message, kind = "error") => send({ type: "notify", message, kind }),
          },
        };
        addEventListener("message", async (event) => {
          if (event.source !== parent) return;
          const message = event.data;
          if (!message || typeof message.type !== "string") return;
          if (message.type === "response") {
            const pendingRequest = pending.get(message.requestId);
            if (!pendingRequest) return;
            pending.delete(message.requestId);
            if (message.ok) pendingRequest.resolve(message.value);
            else pendingRequest.reject(new Error(message.error));
            return;
          }
          if (message.type === "sidebar-event") {
            const rows = registrations.get(message.registrationId);
            const row = rows && rows.find((candidate) => candidate.id === message.rowId);
            const callback = row && row[message.action];
            if (typeof callback === "function") callback(message.selected);
            return;
          }
          if (message.type === "dispose") {
            if (typeof cleanup === "function") cleanup();
            cleanup = null;
            return;
          }
          if (message.type !== "init") return;
          sidebarKeydownRoutingEnabled = message.contribution?.placement?.startsWith("sidebar.") ?? false;
          try {
            const url = URL.createObjectURL(new Blob([message.source], { type: "text/javascript" }));
            const module = await import(url);
            URL.revokeObjectURL(url);
            const entrypoint = module.default || module.pluginEntrypoint;
            if (!entrypoint || typeof entrypoint.mount !== "function") throw new Error("local UI bundle must default-export a PluginUiEntrypoint");
            cleanup = entrypoint.mount(document.body, { plugin: message.plugin, contribution: message.contribution, host });
            send({ type: "mounted" });
          } catch (error) {
            send({ type: "load-error", message: String(error) });
          }
        });
      </scr${"ipt"}>`;

    const registrations = new Map<string, string[]>();
    let unregisterSidebarRows = () => {};
    const rebuildSidebarRows = (): void => {
      unregisterSidebarRows();
      unregisterSidebarRows = registerPluginSidebarContribution(
        `${plugin.id}:${contribution.id}`,
        [...registrations].flatMap(([registrationId, rows]) =>
          rows.map((id) => ({
            id,
            onSelect: () => frame.contentWindow?.postMessage({ type: "sidebar-event", registrationId, rowId: id, action: "onSelect" }, "*"),
            onCollapse: () => frame.contentWindow?.postMessage({ type: "sidebar-event", registrationId, rowId: id, action: "onCollapse" }, "*"),
            onExpand: () => frame.contentWindow?.postMessage({ type: "sidebar-event", registrationId, rowId: id, action: "onExpand" }, "*"),
            onFocus: (selected) => frame.contentWindow?.postMessage({ type: "sidebar-event", registrationId, rowId: id, action: "onFocus", selected }, "*"),
          })),
        ),
      );
    };
    const respond = (requestId: number | undefined, ok: boolean, value?: unknown): void => {
      if (requestId === undefined) return;
      frame.contentWindow?.postMessage(
        ok ? { type: "response", requestId, ok: true, value } : { type: "response", requestId, ok: false, error: String(value) },
        "*",
      );
    };
    const onMessage = (event: MessageEvent<LocalPluginFrameMessage>): void => {
      if (event.source !== frame.contentWindow) return;
      const message = event.data;
      if (!message || typeof message.type !== "string") return;
      if (message.type === "call" && typeof message.method === "string") {
        void callPlugin(message.method, message.params)
          .then((value) => respond(message.requestId, true, value))
          .catch((error) => respond(message.requestId, false, error));
      } else if (message.type === "settings-get") {
        if (!plugin.capabilities.includes("settings")) {
          respond(message.requestId, false, "plugin settings capability is not granted");
          return;
        }
        void plugins
          .settings(plugin.id)
          .then((value) => respond(message.requestId, true, value))
          .catch((error) => respond(message.requestId, false, error));
      } else if (message.type === "settings-replace") {
        if (!plugin.capabilities.includes("settings")) {
          respond(message.requestId, false, "plugin settings capability is not granted");
          return;
        }
        if (!message.params || typeof message.params !== "object" || Array.isArray(message.params)) {
          respond(message.requestId, false, "plugin settings must be a JSON object");
          return;
        }
        void plugins
          .updateSettings(plugin.id, message.params as Record<string, unknown>)
          .then((value) => respond(message.requestId, true, value))
          .catch((error) => respond(message.requestId, false, error));
      } else if (message.type === "data-changed") {
        void plugins
          .dataChanged(plugin.id)
          .then((value) => respond(message.requestId, true, value))
          .catch((error) => respond(message.requestId, false, error));
      } else if (message.type === "navigation") {
        if (message.action === "open" && message.pluginId && message.contributionId) {
          onNavigate(message.pluginId, message.contributionId);
        } else if (message.action === "close") onClose();
        else if (message.action === "preferences") onOpenPreferences();
      } else if (message.type === "notify" && typeof message.message === "string") {
        const notification = message.message.trim();
        if (notification) showSnackbar(notification, message.kind ?? "error");
      } else if (message.type === "sidebar-select" && typeof message.rowId === "string") {
        container?.dispatchEvent(
          new CustomEvent("plugin-sidebar-select", { bubbles: true, detail: { rowId: message.rowId } }),
        );
      } else if (message.type === "sidebar-keydown" && typeof message.key === "string") {
        const detail: { event: KeyboardEvent; handled: boolean } = {
          event: new KeyboardEvent("keydown", {
            key: message.key,
            altKey: message.altKey,
            ctrlKey: message.ctrlKey,
            metaKey: message.metaKey,
            shiftKey: message.shiftKey,
            bubbles: true,
            cancelable: true,
          }),
          handled: false,
        };
        container?.dispatchEvent(
          new CustomEvent("plugin-sidebar-keydown", { bubbles: true, detail }),
        );
      } else if (message.type === "sidebar-register" && message.registrationId && message.rows) {
        registrations.set(
          message.registrationId,
          message.rows.flatMap((row) => (typeof row.id === "string" ? [row.id] : [])),
        );
        rebuildSidebarRows();
      } else if (message.type === "sidebar-unregister" && message.registrationId) {
        if (registrations.delete(message.registrationId)) rebuildSidebarRows();
      } else if (message.type === "load-error") {
        showLoadFailure(root, message.message ?? "local UI bundle failed to load");
      }
    };
    const initialise = (): void => {
      void plugins
        .localUiSource(plugin.id, contribution.id)
        .then((source) => {
          const context = JSON.parse(JSON.stringify({ plugin, contribution })) as {
            plugin: PluginInventory;
            contribution: PluginUiContribution;
          };
          frame.contentWindow?.postMessage({ type: "init", source, ...context }, "*");
        })
        .catch((error) => showLoadFailure(root, error));
    };

    window.addEventListener("message", onMessage);
    frame.addEventListener("load", initialise, { once: true });
    root.replaceChildren(frame);
    return () => {
      window.removeEventListener("message", onMessage);
      unregisterSidebarRows();
      registrations.clear();
      frame.contentWindow?.postMessage({ type: "dispose" }, "*");
      frame.remove();
    };
  }

  async function mountContribution(target: HTMLElement, version: number): Promise<void> {
    disposeCurrent();
    const root = target.shadowRoot ?? target.attachShadow({ mode: "open" });
    if (plugin.state !== "running") {
      root.replaceChildren();
      return;
    }
    try {
      if (plugin.source_kind !== "builtin") {
        const cleanup = createLocalPluginFrame(root);
        if (version !== generation) {
          cleanup();
          return;
        }
        disposer = cleanup;
        if (autofocus) target.focus();
        return;
      }
      const entrypoint = await loadBundledEntrypoint();
      if (version !== generation) return;
      const data: PluginUiHost["data"] = {
        changed: () => plugins.dataChanged(plugin.id),
        notify: (message, kind = "error") => showSnackbar(message, kind),
      };
      const host: PluginUiHost = {
        call: <T>(method: string, params: unknown = null) => callPlugin<T>(method, params),
        settings: {
          get: <T extends Record<string, unknown>>() => {
            if (!plugin.capabilities.includes("settings")) {
              return Promise.reject(new Error("plugin settings capability is not granted"));
            }
            return plugins.settings<T>(plugin.id);
          },
          replace: <T extends Record<string, unknown>>(settings: T) => {
            if (!plugin.capabilities.includes("settings")) {
              return Promise.reject(new Error("plugin settings capability is not granted"));
            }
            return plugins.updateSettings<T>(plugin.id, settings);
          },
        },
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
        data,
      };
      data.refreshAssignment = async (project) => {
        try {
          await Promise.all([taskStore.refresh([project.path]), plugins.dataChanged(plugin.id)]);
        } catch (error) {
          showSnackbar(`Child task created, but refresh failed: ${String(error)}`);
        }
      };
      data.onChanged = (listener) => subscribe(dataChangeListeners, listener);
      data.onTaskDataChanged = (listener) => subscribe(taskDataChangeListeners, listener);
      host.projects = {
        list: async () => (await projectsApi.list()).filter((project) => !project.hidden),
      };
      host.tasks = {
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
      };
      host.interaction = {
        openModal: openPluginModal,
        openProjectForm,
      };
      const cleanup = entrypoint.mount(root, { plugin, contribution, host });
      if (typeof cleanup !== "function") {
        throw new Error("plugin UI entrypoint mount must return a disposer function");
      }

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

  $effect(() => {
    if (plugin.id !== "jira" || contribution.placement !== "sidebar.section") return;
    getAllTasks();
    notify(taskDataChangeListeners);
  });

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<string>("plugin-data-changed", (event) => {
      if (event.payload !== plugin.id || !["sidebar.section", "interaction"].includes(contribution.placement)) return;
      if (plugin.source_kind === "builtin" && dataChangeListeners.size > 0) {
        notify(dataChangeListeners);
      } else {
        retry();
      }
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
  class={
    contribution.placement === "interaction"
      ? plugin.source_kind === "builtin"
        ? "pointer-events-none"
        : "h-full w-full pointer-events-auto"
      : contribution.placement === "main-pane"
        ? "h-full w-full"
        : "w-full"
  }
  tabindex="-1"
  role="region"
  aria-label={`${plugin.name} · ${contribution.label}`}
  data-plugin-ui-contribution={`${plugin.id}:${contribution.id}`}
  data-plugin-sidebar-contribution={contribution.placement.startsWith("sidebar.") ? "" : undefined}
  bind:this={container}
  onfocus={() => {
    if (contribution.placement === "interaction") {
      container?.shadowRoot?.querySelector<HTMLElement>("[data-plugin-interaction]")?.focus();
    }
  }}
></div>
