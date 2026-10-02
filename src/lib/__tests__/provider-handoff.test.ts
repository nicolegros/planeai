import { beforeEach, describe, expect, it, vi } from "vitest";

const { handoff, handback } = vi.hoisted(() => ({
  handoff: vi.fn(async () => ["/opt/claude", "--resume", "s1", "--permission-mode", "plan"]),
  handback: vi.fn(async () => {}),
}));
vi.mock("../api", () => ({ providerSessions: { handoff, handback } }));
const { showSnackbar } = vi.hoisted(() => ({ showSnackbar: vi.fn() }));
vi.mock("../snackbar.svelte", () => ({ showSnackbar }));

import {
  forgetHandoff,
  handoffTabFor,
  quoteArgument,
  shellCommand,
  shellTabClosed,
  startHandoff,
} from "../provider-handoff";

describe("provider handoff", () => {
  beforeEach(() => vi.clearAllMocks());

  it("quotes arguments for POSIX shells and cmd.exe", () => {
    expect(shellCommand(["/opt/claude", "--resume", "s1"], false)).toBe("/opt/claude --resume s1");
    expect(quoteArgument("it's here", false)).toBe(`'it'\\''s here'`);
    expect(quoteArgument("C:\\Program Files\\claude.exe", true)).toBe(
      '"C:\\Program Files\\claude.exe"',
    );
    expect(quoteArgument("plain", true)).toBe("plain");
  });

  it("opens one terminal tab per session and hands back when it closes", async () => {
    const open = vi.fn(() => "s1:4");
    await expect(startHandoff("s1", open)).resolves.toBe("s1:4");
    expect(open).toHaveBeenCalledWith("/opt/claude --resume s1 --permission-mode plan", "Terminal");
    await expect(startHandoff("s1", open)).resolves.toBe("s1:4");
    expect(handoff).toHaveBeenCalledOnce();
    expect(handoffTabFor("s1")).toBe("s1:4");

    await shellTabClosed("s2:1");
    expect(handback).not.toHaveBeenCalled();
    await shellTabClosed("s1:4");
    expect(handback).toHaveBeenCalledWith("s1");
    expect(handoffTabFor("s1")).toBeUndefined();
  });

  it("opens a single tab when the handoff is requested twice before it lands", async () => {
    const open = vi.fn(() => "s4:1");
    const [first, second] = await Promise.all([startHandoff("s4", open), startHandoff("s4", open)]);
    expect([first, second]).toEqual(["s4:1", "s4:1"]);
    expect(open).toHaveBeenCalledOnce();
    expect(handoff).toHaveBeenCalledOnce();
  });

  it("closes the tab even when the session cannot return to the chat", async () => {
    await startHandoff("s5", () => "s5:1");
    handback.mockRejectedValueOnce(new Error("plugin is not running"));
    await expect(shellTabClosed("s5:1")).resolves.toBeUndefined();
    expect(showSnackbar).toHaveBeenCalledWith(
      expect.stringContaining("could not return to the chat"),
    );
    expect(handoffTabFor("s5")).toBeUndefined();
  });

  it("opens a fresh tab after forgetting a handoff whose tab vanished", async () => {
    await startHandoff("s6", () => "s6:1");
    forgetHandoff("s6");
    expect(handoffTabFor("s6")).toBeUndefined();
    await expect(startHandoff("s6", () => "s6:2")).resolves.toBe("s6:2");
  });

  it("returns the session to the chat when no pane can hold the terminal", async () => {
    await expect(startHandoff("s3", () => null)).rejects.toThrow("no pane");
    expect(handback).toHaveBeenCalledWith("s3");
    expect(handoffTabFor("s3")).toBeUndefined();
  });
});
