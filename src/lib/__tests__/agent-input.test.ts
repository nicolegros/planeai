import { beforeEach, describe, expect, it, vi } from "vitest";

const { pty, providerSessions } = vi.hoisted(() => ({
  pty: {
    write: vi.fn(async (_id: string, _bytes: number[]) => true),
    isProgramRunning: vi.fn(async (_key: string) => true),
  },
  providerSessions: { send: vi.fn(async (_id: string, _text: string) => {}) },
}));
vi.mock("../api", () => ({ pty, providerSessions }));
const { tabFor } = vi.hoisted(() => ({
  tabFor: vi.fn((_id: string): string | undefined => undefined),
}));
vi.mock("../task-workspace-layout.svelte", () => ({ providerHandoff: { tabFor } }));

import { sendToAgent } from "../agent-input";

const pasted = (text: string) => Array.from(new TextEncoder().encode(`\x1b[200~${text}\x1b[201~`));

describe("sendToAgent", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sends a chat session's message through its provider", async () => {
    await sendToAgent({ id: "s1", backend: "plugin" }, "review notes");
    expect(providerSessions.send).toHaveBeenCalledWith("s1", "review notes");
    expect(pty.write).not.toHaveBeenCalled();
  });

  it("pastes a terminal session's message as one bracketed paste, then presses Enter", async () => {
    await sendToAgent({ id: "s1", backend: "local" }, "first comment\nsecond comment");
    expect(pty.write.mock.calls).toEqual([
      ["s1", pasted("first comment\nsecond comment")],
      ["s1", [0x0d]],
    ]);
  });

  it("keeps a paste end marker in the message from ending the paste early", async () => {
    await sendToAgent({ id: "s1", backend: "local" }, "code\x1b[201~\nrest");
    expect(pty.write.mock.calls[0]).toEqual(["s1", pasted("code\nrest")]);
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
      ["s1:3", pasted("hi")],
      ["s1:3", [0x0d]],
    ]);
  });

  it("never types into a handoff terminal whose agent is gone", async () => {
    tabFor.mockReturnValueOnce("s1:3");
    pty.isProgramRunning.mockResolvedValueOnce(false);
    await expect(sendToAgent({ id: "s1", backend: "plugin" }, "$(rm -rf ~)")).rejects.toThrow(
      "no longer runs its agent",
    );
    expect(pty.write).not.toHaveBeenCalled();
  });

  it("fails when a provider refuses the message", async () => {
    providerSessions.send.mockRejectedValueOnce(new Error("plugin is not running"));
    await expect(sendToAgent({ id: "s1", backend: "plugin" }, "hi")).rejects.toThrow("not running");
  });
});
