import { describe, expect, it, vi } from "vitest";

vi.mock("../api", () => ({ providerSessions: {} }));
vi.mock("../snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import { createProviderHandoff, quoteArgument, shellCommand } from "../provider-handoff";

function setup() {
  const api = {
    handoff: vi.fn(async (_sessionId: string) => [
      "/opt/claude",
      "--resume",
      "s1",
      "--permission-mode",
      "plan",
    ]),
    handback: vi.fn(async (_sessionId: string) => {}),
  };
  const notify = vi.fn();
  return { api, notify, handoff: createProviderHandoff({ api, notify }) };
}

describe("provider handoff", () => {
  it("quotes arguments for POSIX shells and cmd.exe", () => {
    expect(shellCommand(["/opt/claude", "--resume", "s1"], false)).toBe("/opt/claude --resume s1");
    expect(quoteArgument("it's here", false)).toBe(`'it'\\''s here'`);
    expect(quoteArgument("C:\\Program Files\\claude.exe", true)).toBe(
      '"C:\\Program Files\\claude.exe"',
    );
    expect(quoteArgument("plain", true)).toBe("plain");
  });

  it("keeps cmd.exe from expanding variables or merging arguments", () => {
    // `^%` outside quotes is a literal percent sign to cmd.exe.
    expect(quoteArgument("50%PATH%", true)).toBe('"50"^%"PATH"^%""');
    // Backslashes before the closing quote are doubled, so it still closes the argument.
    expect(quoteArgument("C:\\my dir\\", true)).toBe('"C:\\my dir\\\\"');
    expect(quoteArgument('say \\"hi"', true)).toBe('"say \\\\""hi"""');
  });

  it("opens one terminal tab per session and hands back when it closes", async () => {
    const { api, handoff } = setup();
    const open = vi.fn(() => "s1:4");
    await expect(handoff.start("s1", open)).resolves.toBe("s1:4");
    expect(open).toHaveBeenCalledWith("/opt/claude --resume s1 --permission-mode plan", "Terminal");
    await expect(handoff.start("s1", open)).resolves.toBe("s1:4");
    expect(api.handoff).toHaveBeenCalledOnce();
    expect(handoff.tabFor("s1")).toBe("s1:4");

    await handoff.shellClosed("s2:1");
    expect(api.handback).not.toHaveBeenCalled();
    await handoff.shellClosed("s1:4");
    expect(api.handback).toHaveBeenCalledWith("s1");
    expect(handoff.tabFor("s1")).toBeUndefined();
  });

  it("opens a single tab when the handoff is requested twice before it lands", async () => {
    const { api, handoff } = setup();
    const open = vi.fn(() => "s1:1");
    const results = await Promise.all([handoff.start("s1", open), handoff.start("s1", open)]);
    expect(results).toEqual(["s1:1", "s1:1"]);
    expect(open).toHaveBeenCalledOnce();
    expect(api.handoff).toHaveBeenCalledOnce();
  });

  it("closes the tab even when the session cannot return to the chat", async () => {
    const { api, notify, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    api.handback.mockRejectedValueOnce(new Error("plugin is not running"));
    await expect(handoff.shellClosed("s1:1")).resolves.toBeUndefined();
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("could not return to the chat"));
    expect(handoff.tabFor("s1")).toBeUndefined();
  });

  it("returns the session to the chat when no pane can hold the terminal", async () => {
    const { api, handoff } = setup();
    await expect(handoff.start("s1", () => null)).rejects.toThrow("no pane");
    expect(api.handback).toHaveBeenCalledWith("s1");
    expect(handoff.tabFor("s1")).toBeUndefined();
  });

  it("ends a handoff by closing its tab, which hands the session back", async () => {
    const { api, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    const closeTab = vi.fn(async (ptyKey: string) => {
      await handoff.shellClosed(ptyKey);
      return "closed" as const;
    });
    await handoff.end("s1", closeTab);
    expect(closeTab).toHaveBeenCalledWith("s1:1");
    expect(api.handback).toHaveBeenCalledOnce();
  });

  it("hands back directly, and forgets the tab, when the tab is already gone", async () => {
    const { api, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    await handoff.end("s1", async () => "missing");
    expect(api.handback).toHaveBeenCalledWith("s1");
    expect(handoff.tabFor("s1")).toBeUndefined();
    await expect(handoff.start("s1", () => "s1:2")).resolves.toBe("s1:2");
  });

  it("hands back a session without a tab", async () => {
    const { api, handoff } = setup();
    const closeTab = vi.fn();
    await handoff.end("s1", closeTab);
    expect(closeTab).not.toHaveBeenCalled();
    expect(api.handback).toHaveBeenCalledWith("s1");
  });

  it("waits for a handoff still starting, so the chat and the terminal never both drive", async () => {
    const { api, handoff } = setup();
    let releaseHandoff!: (argv: string[]) => void;
    api.handoff.mockReturnValueOnce(new Promise((resolve) => (releaseHandoff = resolve)));
    const started = handoff.start("s1", () => "s1:1");
    const closeTab = vi.fn(async (ptyKey: string) => {
      await handoff.shellClosed(ptyKey);
      return "closed" as const;
    });
    const ended = handoff.end("s1", closeTab);
    releaseHandoff(["claude"]);
    await started;
    await ended;
    expect(closeTab).toHaveBeenCalledWith("s1:1");
    expect(api.handback).toHaveBeenCalledOnce();
    expect(handoff.tabFor("s1")).toBeUndefined();
  });

  it("keeps the handoff while its terminal is still starting", async () => {
    const { api, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    await expect(handoff.end("s1", async () => "starting")).rejects.toThrow("still starting");
    expect(api.handback).not.toHaveBeenCalled();
    expect(handoff.tabFor("s1")).toBe("s1:1");
  });
});
