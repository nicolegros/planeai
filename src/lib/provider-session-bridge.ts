import { listen } from "@tauri-apps/api/event";
import { plugins, providerSessions } from "./api";
import type { PluginSessionEvent } from "./plugin-sdk";
import type { ProviderSessionEvent } from "./types";

/**
 * What PlaneAI offers a provider session's UI: its session's controls and events.
 * Plugin frames receive these only through the host, which owns this object.
 */
export interface ProviderSessionBridge {
  /** The UI bundle, once the current sidecar drives the session. */
  loadSource(): Promise<string>;
  send(text: string): Promise<void>;
  interrupt(): Promise<void>;
  /** Present when the provider can continue the session in a terminal. */
  handoff?: () => Promise<void>;
  handback?: () => Promise<void>;
  /** Resolves to an unsubscribe once listening. */
  subscribe(listener: (event: PluginSessionEvent) => void): Promise<() => void>;
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

const CONTROLS_REFUSED = "session controls are available only to provider session UIs";
const HANDOFF_REFUSED = "terminal handoff is not available here";

/** Session requests a frame may post, each with why it is refused when unavailable. */
const SESSION_REQUESTS: Record<
  string,
  {
    refusal: string;
    run: (bridge: ProviderSessionBridge, message: { text?: unknown }) => Promise<void> | undefined;
  }
> = {
  "session-send": {
    refusal: CONTROLS_REFUSED,
    run: (bridge, { text }) =>
      typeof text === "string" ? bridge.send(text) : Promise.reject("session.send requires text"),
  },
  "session-interrupt": { refusal: CONTROLS_REFUSED, run: (bridge) => bridge.interrupt() },
  "session-handoff": { refusal: HANDOFF_REFUSED, run: (bridge) => bridge.handoff?.() },
  "session-handback": { refusal: HANDOFF_REFUSED, run: (bridge) => bridge.handback?.() },
};

/** Frames can post anything, so only the table's own keys are session requests. */
export function isSessionRequest(type: unknown): type is string {
  return typeof type === "string" && Object.hasOwn(SESSION_REQUESTS, type);
}

/** Serve a frame's session request from its bridge; refused when it has none. */
export function serveSessionRequest(
  bridge: ProviderSessionBridge | undefined,
  message: { type: string; text?: unknown },
): Promise<void> {
  const request = SESSION_REQUESTS[message.type];
  return (bridge && request.run(bridge, message)) ?? Promise.reject(request.refusal);
}
