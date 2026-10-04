import { beforeEach, describe, expect, it, vi } from "vitest";

const { pty, providerSessions } = vi.hoisted(() => ({
  pty: { write: vi.fn(async (_id: string, _bytes: number[]) => true) },
  providerSessions: { send: vi.fn(async (_id: string, _text: string) => {}) },
}));
vi.mock("../api", () => ({ pty, providerSessions }));
const { tabFor } = vi.hoisted(() => ({
  tabFor: vi.fn((_id: string): string | undefined => undefined),
}));
vi.mock("../task-workspace-layout.svelte", () => ({ providerHandoff: { tabFor } }));

import { sendToAgent } from "../agent-input";

describe("sendToAgent", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sends a chat session's message through its provider", async () => {
    await sendToAgent({ id: "s1", backend: "plugin" }, "review notes");
    expect(providerSessions.send).toHaveBeenCalledWith("s1", "review notes");
    expect(pty.write).not.toHaveBeenCalled();
  });

  it("types a terminal session's message, then presses Enter", async () => {
    await sendToAgent({ id: "s1", backend: "local" }, "hi");
    expect(pty.write.mock.calls).toEqual([
      ["s1", [0x68, 0x69]],
      ["s1", [0x0d]],
    ]);
  });

  it("fails when the terminal is not attached, so nothing is reported as sent", async () => {
    pty.write.mockResolvedValueOnce(false);
    await expect(sendToAgent({ id: "s1", backend: "local" }, "hi")).rejects.toThrow("not attached");
    expect(pty.write).toHaveBeenCalledOnce();
  });

  it("types a chat's message into the terminal it continues in", async () => {
    tabFor.mockReturnValueOnce("s1:3");
    await sendToAgent({ id: "s1", backend: "plugin" }, "hi");
    expect(providerSessions.send).not.toHaveBeenCalled();
    expect(pty.write.mock.calls).toEqual([
      ["s1:3", [0x68, 0x69]],
      ["s1:3", [0x0d]],
    ]);
  });

  it("fails when a provider refuses the message", async () => {
    providerSessions.send.mockRejectedValueOnce(new Error("plugin is not running"));
    await expect(sendToAgent({ id: "s1", backend: "plugin" }, "hi")).rejects.toThrow("not running");
  });
});
