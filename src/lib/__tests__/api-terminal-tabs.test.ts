import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: class {} }));

import { pty } from "../api";

describe("pty terminal tabs", () => {
  it("opens a tab with what it runs", async () => {
    await pty.openTab("s1", { kind: "program", argv: ["claude", "--resume", "it's 100%"] });
    expect(invoke).toHaveBeenLastCalledWith("open_tab", {
      sessionId: "s1",
      spec: { kind: "program", argv: ["claude", "--resume", "it's 100%"] },
    });
  });

  it("attaches to a tab by index, without restating what it runs", async () => {
    const channel = {} as never;
    await pty.attachTab("s1", 2, true, channel);
    expect(invoke).toHaveBeenLastCalledWith("attach_tab", {
      sessionId: "s1",
      tabIndex: 2,
      darkMode: true,
      onData: channel,
    });
  });

  it("asks which tabs ended", async () => {
    await pty.endedTabs(["s1:2", "s1:3"]);
    expect(invoke).toHaveBeenLastCalledWith("ended_tabs", { ptyKeys: ["s1:2", "s1:3"] });
  });
});
