import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: class {} }));

import { pty } from "../api";

describe("pty.spawnTab", () => {
  const channel = {} as never;

  it("hands a command line to the tab's shell", async () => {
    await pty.spawnTab("s1", 2, true, channel, "nvim src/main.rs");
    expect(invoke).toHaveBeenLastCalledWith(
      "spawn_tab",
      expect.objectContaining({ initialCommand: "nvim src/main.rs", initialArgv: undefined }),
    );
  });

  it("runs a program and its arguments without a shell", async () => {
    await pty.spawnTab("s1", 2, true, channel, ["claude", "--resume", "it's 100%"]);
    expect(invoke).toHaveBeenLastCalledWith(
      "spawn_tab",
      expect.objectContaining({
        initialCommand: undefined,
        initialArgv: ["claude", "--resume", "it's 100%"],
      }),
    );
  });
});
