import type {
  PluginInventory,
  PluginProvider,
  PluginUiContribution,
  Project,
  TaskItem,
} from "./types";
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
  /** Resolves the currently focused session when invoked, rather than mount context. */
  recipient: {
    getFocusedAgentSession(): Promise<PluginSessionContext | null>;
  };
  /**
   * Calls a PlaneAI public data operation directly. The host derives the owning
   * plugin identity, checks its manifest capability, and never exposes Tauri.
   */
  rpc: {
    call<T>(method: string, params?: unknown): Promise<T>;
  };
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
    openExternal(url: string): void;
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
  /**
   * Present only for a provider's session UI. Input goes through the host so CLI,
   * recipes and the UI share one delivery path; events are the sidecar's own
   * `host.providerSession.event` payloads, in `seq` order, for this session only.
   * Failed requests reject with an `Error` whose `code` is a {@link PluginSessionErrorCode}.
   */
  session?: {
    send(text: string): Promise<void>;
    interrupt(): Promise<void>;
    onEvent(listener: (event: PluginSessionEvent) => void): () => void;
    /**
     * Present when the provider supports `handoff`: continue the session in its agent's
     * TUI in a terminal tab. `handback` closes that tab and returns to the chat.
     */
    handoff?(): Promise<void>;
    handback?(): Promise<void>;
  };
}

/** Why a session request failed, as the host's `ProviderErrorCode` names it. */
export const PLUGIN_SESSION_ERROR_CODES = [
  "handed_off",
  "prompt_too_large",
  "not_running",
  "unsupported",
  "unavailable",
  "plugin_error",
] as const;

export type PluginSessionErrorCode = (typeof PLUGIN_SESSION_ERROR_CODES)[number];

/** What a provider's session UI receives in `context.provider`. */
export type PluginProviderContext = Pick<PluginProvider, "id" | "label" | "supports">;

export interface PluginSessionEvent {
  seq: number;
  payload: unknown;
}

export interface PluginSessionContext {
  id: string;
  projectId: string;
  branch: string;
  baseBranch: string | null;
  status: "active" | "exited" | "archived";
  provider: string | null;
  taskKey: string | null;
}

export interface PluginUiContext {
  plugin: PluginInventory;
  contribution: PluginUiContribution;
  /** Present for session.main (a provider's session UI), session.panel, session.indicator, and titlebar contributions. */
  session?: PluginSessionContext;
  /** Present for a provider's session UI: the provider the session runs on. */
  provider?: PluginProviderContext;
  host: PluginUiHost;
}

export type PluginUiDisposer = () => void;

export interface PluginUiEntrypoint {
  mount(root: ShadowRoot, context: PluginUiContext): PluginUiDisposer;
}
