import { afterEach, describe, expect, it, vi } from "vitest";

const tauriEvent = vi.hoisted(() => ({
  handler: undefined as (() => void) | undefined,
  unlisten: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriEvent.listen,
}));

import { subscribeToJiraConnectionState } from "../jira-connection-state";

describe("Jira connection-state subscription", () => {
  afterEach(() => {
    vi.clearAllMocks();
    tauriEvent.handler = undefined;
  });

  it("refreshes sidebar presentation after reconnect-required is emitted", async () => {
    const refresh = vi.fn();
    tauriEvent.listen.mockImplementation((_event, handler) => {
      tauriEvent.handler = handler;
      return Promise.resolve(tauriEvent.unlisten);
    });

    const stop = subscribeToJiraConnectionState(refresh);
    await Promise.resolve();

    expect(tauriEvent.listen).toHaveBeenCalledWith(
      "jira-connection-state-changed",
      expect.any(Function),
    );
    tauriEvent.handler?.();
    expect(refresh).toHaveBeenCalledOnce();

    stop();
    expect(tauriEvent.unlisten).toHaveBeenCalledOnce();
  });

  it("handles a clear interleaved with listener setup before loading initial status", async () => {
    const events: string[] = [];
    tauriEvent.listen.mockImplementation((_event, handler) => {
      tauriEvent.handler = handler;
      // The backend clears OAuth after registration but before the listener promise resolves.
      handler();
      return Promise.resolve(tauriEvent.unlisten);
    });

    subscribeToJiraConnectionState(
      () => events.push("clear"),
      () => events.push("initial-status"),
    );
    await Promise.resolve();

    expect(events).toEqual(["clear", "initial-status"]);
  });
});
