import type { PluginInventory, PluginUiContribution, Project, TaskItem } from "./types";
import type { PluginSidebarNavRow } from "./plugin-sidebar-navigation.svelte";

export interface PluginModalControls {
  close(): void;
  dispose(): void;
  setSubmitting(submitting: boolean): void;
}

export interface PluginModalOptions {
  title: string;
  contentResponsive?: boolean;
  mount(root: ShadowRoot, controls: PluginModalControls): (() => void) | void;
}

/**
 * Plugin UI modules use only this host-injected bridge. The host scopes every
 * request to the selected plugin runtime; modules never invoke Tauri directly.
 */
export type PluginSettings = Record<string, unknown>;

export interface PluginUiHost {
  /** Calls the owning plugin sidecar; lifecycle methods remain reserved for PlaneAI. */
  call<T>(method: string, params?: unknown): Promise<T>;
  /**
   * Public, JSON-object settings shared with the plugin sidecar's capability-gated
   * `host.settings.get` and `host.settings.replace` callbacks. Secrets are never
   * available through this bridge.
   */
  settings: {
    get<T extends PluginSettings = PluginSettings>(): Promise<T>;
    replace<T extends PluginSettings = PluginSettings>(settings: T): Promise<T>;
  };
  navigation: {
    open(pluginId: string, contributionId: string): void;
    close(): void;
    openPreferences(): void;
  };
  sidebar: {
    register(rows: PluginSidebarNavRow[]): () => void;
    select(rowId: string): void;
    handleKeydown(event: KeyboardEvent): void;
  };
  data: {
    changed(): Promise<void>;
    onChanged?(listener: () => void): () => void;
    onTaskDataChanged?(listener: () => void): () => void;
    refreshAssignment?(project: Project): Promise<void>;
    notify(message: string, kind?: "success" | "error"): void;
  };
  projects?: {
    list(): Promise<Project[]>;
  };
  tasks?: {
    createChild(params: {
      project: Project;
      title: string;
      description: string;
      parentKey: string;
    }): Promise<TaskItem>;
  };
  interaction?: {
    openModal(options: PluginModalOptions): PluginModalControls;
    openProjectForm(): Promise<Project | null>;
  };
}

export interface PluginUiContext {
  plugin: PluginInventory;
  contribution: PluginUiContribution;
  host: PluginUiHost;
}

export type PluginUiDisposer = () => void;

export interface PluginUiEntrypoint {
  mount(root: ShadowRoot, context: PluginUiContext): PluginUiDisposer;
}
