import { listen } from "@tauri-apps/api/event";
import { plugins, providerSessions } from "./api";
import type { ProviderSessionEvent } from "./types";

/** One opaque event from a provider to its session's UI, in the provider's own order. */
export interface ProviderSessionEventPayload {
  seq: number;
  payload: unknown;
}

/**
 * What PlaneAI offers a provider session's UI: its session's controls and events.
 * Plugin frames receive these only through the host, which owns this object.
 */
export interface ProviderSessionBridge {
  readonly sessionId: string;
  /** The UI bundle, once the current sidecar drives the session. */
  loadSource(): Promise<string>;
  send(text: string): Promise<void>;
  interrupt(): Promise<void>;
  /** Present when the provider can continue the session in a terminal. */
  handoff?: () => Promise<void>;
  handback?: () => Promise<void>;
  /** Resolves to an unsubscribe once listening. */
  subscribe(listener: (event: ProviderSessionEventPayload) => void): Promise<() => void>;
}

export function createProviderSessionBridge(options: {
  pluginId: string;
  providerId: string;
  sessionId: string;
  handoff?: () => Promise<void>;
  handback?: () => Promise<void>;
}): ProviderSessionBridge {
  const { pluginId, providerId, sessionId } = options;
  return {
    sessionId,
    loadSource: () =>
      providerSessions
        .ensure(sessionId)
        .then(() => plugins.localProviderUiSource(pluginId, providerId)),
    send: (text) => providerSessions.send(sessionId, text),
    interrupt: () => providerSessions.interrupt(sessionId),
    handoff: options.handoff,
    handback: options.handback,
    subscribe: (listener) =>
      listen<ProviderSessionEvent>("plugin-provider-session-event", ({ payload }) => {
        // Events from another plugin, or for another session, never reach this UI.
        if (payload.plugin_id !== pluginId || payload.session_id !== sessionId) return;
        listener({ seq: payload.seq, payload: payload.payload });
      }),
  };
}
