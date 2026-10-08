import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../api", () => ({ plugins: {}, providerSessions: {} }));

import {
  isSessionRequest,
  serveSessionRequest,
  type ProviderSessionBridge,
} from "../provider-session-bridge";

function bridge(overrides: Partial<ProviderSessionBridge> = {}): ProviderSessionBridge {
  return {
    provider: { id: "echo", label: "Echo", entrypoint: "ui/chat.js", supports: [] },
    loadSource: vi.fn(),
    send: vi.fn(async () => {}),
    interrupt: vi.fn(async () => {}),
    subscribe: vi.fn(),
    ...overrides,
  };
}

describe("serveSessionRequest", () => {
  it("recognizes only session requests, never inherited keys", () => {
    for (const type of ["data-changed", "toString", "constructor", "__proto__", 7, undefined]) {
      expect(isSessionRequest(type)).toBe(false);
    }
    expect(isSessionRequest("session-send")).toBe(true);
  });

  it("serves the session's controls from its bridge", async () => {
    const session = bridge();
    await serveSessionRequest(session, { type: "session-send", text: "hi" });
    await serveSessionRequest(session, { type: "session-interrupt" });
    expect(session.send).toHaveBeenCalledWith("hi");
    expect(session.interrupt).toHaveBeenCalledOnce();
  });

  it("requires text to send", async () => {
    await expect(serveSessionRequest(bridge(), { type: "session-send" })).rejects.toThrow(
      "session.send requires text",
    );
  });

  it("refuses session controls to frames without a provider session", async () => {
    await expect(
      serveSessionRequest(undefined, { type: "session-send", text: "hi" }),
    ).rejects.toMatchObject({
      code: "unsupported",
      message: "session controls are available only to provider session UIs",
    });
  });

  it("refuses handoff when the provider cannot continue in a terminal", async () => {
    await expect(serveSessionRequest(bridge(), { type: "session-handoff" })).rejects.toMatchObject({
      code: "unsupported",
      message: "terminal handoff is not available here",
    });
    const handback = vi.fn(async () => {});
    await serveSessionRequest(bridge({ handback }), { type: "session-handback" });
    expect(handback).toHaveBeenCalledOnce();
  });
});
