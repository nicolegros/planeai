import { describe, expect, it, vi } from "vitest";

vi.mock("../api", () => ({ providerSessions: {} }));
vi.mock("../snackbar.svelte", () => ({ showSnackbar: vi.fn() }));

import { createProviderHandoff } from "../provider-handoff";

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
  const discardTab = vi.fn(async (_ptyKey: string) => true);
  const closeTerminal = vi.fn(async (_ptyKey: string) => {});
  const isProgramRunning = vi.fn(async (_ptyKey: string) => true);
  let time = 0;
  const clock = { advance: (ms: number) => (time += ms) };
  const handoff = createProviderHandoff({
    api,
    discardTab,
    closeTerminal,
    isProgramRunning,
    notify,
    now: () => time,
  });
  return { api, notify, discardTab, closeTerminal, isProgramRunning, clock, handoff };
}

describe("provider handoff", () => {
  it("opens one terminal tab per session and hands back when it closes", async () => {
    const { api, handoff } = setup();
    const open = vi.fn(() => "s1:4");
    await expect(handoff.start("s1", open)).resolves.toBe("s1:4");
    // The argv runs as is: nothing a shell could misread.
    expect(open).toHaveBeenCalledWith(
      ["/opt/claude", "--resume", "s1", "--permission-mode", "plan"],
      "Terminal",
    );
    await expect(handoff.start("s1", open)).resolves.toBe("s1:4");
    expect(api.handoff).toHaveBeenCalledOnce();
    expect(handoff.tabFor("s1")).toBe("s1:4");
    expect(handoff.isHandoffTab("s1:4")).toBe(true);
    expect(handoff.isHandoffTab("s1:5")).toBe(false);

    await handoff.shellClosed("s2:1", false);
    expect(api.handback).not.toHaveBeenCalled();
    await handoff.shellClosed("s1:4", false);
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
    await expect(handoff.shellClosed("s1:1", false)).resolves.toBeUndefined();
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
      await handoff.shellClosed(ptyKey, false);
      return "closed" as const;
    });
    await handoff.end("s1", closeTab);
    expect(closeTab).toHaveBeenCalledWith("s1:1");
    expect(api.handback).toHaveBeenCalledOnce();
  });

  it("keeps a terminal it cannot close here, which still drives the session", async () => {
    const { api, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    await expect(handoff.end("s1", async () => "missing")).rejects.toThrow(
      "Close the session's terminal",
    );
    expect(api.handback).not.toHaveBeenCalled();
    expect(handoff.tabFor("s1")).toBe("s1:1");
    // Closing it later, wherever it is, still hands the session back.
    await handoff.shellClosed("s1:1", false);
    expect(api.handback).toHaveBeenCalledWith("s1");
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
      await handoff.shellClosed(ptyKey, false);
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

  it("explains a terminal that exits right after it opens", async () => {
    const { notify, clock, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    clock.advance(800);
    await handoff.shellClosed("s1:1", true);
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("closed right after it opened"));
  });

  it("does not claim the session returned when its handback failed", async () => {
    const { api, notify, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    api.handback.mockRejectedValueOnce(new Error("plugin is not running"));
    await handoff.shellClosed("s1:1", true);
    expect(notify.mock.calls).toEqual([[expect.stringContaining("could not return to the chat")]]);
  });

  it("stays quiet when the user closes the terminal or the TUI ran a while", async () => {
    const { notify, clock, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    await handoff.shellClosed("s1:1", false);
    await handoff.start("s2", () => "s2:1");
    clock.advance(60_000);
    await handoff.shellClosed("s2:1", true);
    expect(notify).not.toHaveBeenCalled();
  });

  it("closes the terminal of a session that leaves the app, so nothing drives it unseen", async () => {
    const { api, discardTab, closeTerminal, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    await handoff.release("s1");
    expect(discardTab).toHaveBeenCalledWith("s1:1");
    expect(closeTerminal).not.toHaveBeenCalled();
    expect(handoff.tabFor("s1")).toBeUndefined();
    // Restored later, it returns to the chat and can hand off again.
    await handoff.end("s1", vi.fn());
    expect(api.handback).toHaveBeenCalledWith("s1");
    await expect(handoff.start("s1", () => "s1:2")).resolves.toBe("s1:2");
  });

  it("ends the process of a terminal whose workspace is not loaded", async () => {
    const { discardTab, closeTerminal, handoff } = setup();
    await handoff.start("s1", () => "s1:1");
    discardTab.mockResolvedValueOnce(false);
    await handoff.release("s1");
    expect(closeTerminal).toHaveBeenCalledWith("s1:1");
  });

  it("closes a terminal that opens after its session left the app", async () => {
    const { api, discardTab, handoff } = setup();
    let answer!: (argv: string[]) => void;
    api.handoff.mockReturnValueOnce(new Promise((resolve) => (answer = resolve)));
    const started = handoff.start("s1", () => "s1:1");
    const released = handoff.release("s1");
    answer(["claude"]);
    await started;
    await released;
    expect(discardTab).toHaveBeenCalledWith("s1:1");
    expect(handoff.tabFor("s1")).toBeUndefined();
  });

  it("adopts a restored handoff terminal, whose close hands its session back", async () => {
    const { api, notify, handoff } = setup();
    await handoff.adopt("s1:3");
    await handoff.adopt("s1:diff");
    expect(handoff.tabFor("s1")).toBe("s1:3");
    // Its exit right after the reload says nothing about the TUI failing to start.
    await handoff.shellClosed("s1:3", true);
    expect(api.handback).toHaveBeenCalledWith("s1");
    expect(notify).not.toHaveBeenCalled();
  });

  it("never adopts a restored tab whose program is gone, which is a plain shell", async () => {
    // After an app restart the TUI died with the app: prompts must not be typed into a shell.
    const { isProgramRunning, handoff } = setup();
    isProgramRunning.mockResolvedValueOnce(false);
    await handoff.adopt("s1:3");
    expect(handoff.tabFor("s1")).toBeUndefined();
  });
});
