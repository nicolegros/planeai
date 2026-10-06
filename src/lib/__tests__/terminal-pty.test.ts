import { describe, expect, it, vi } from "vitest";

const { attachTab } = vi.hoisted(() => ({ attachTab: vi.fn(async () => true) }));
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {} }));
vi.mock("../api", () => ({ pty: { attachTab } }));
vi.mock("../settings.svelte", () => ({ isDark: () => true }));

import { TabEndedError, tauriTerminalPty } from "../terminal-pty";

describe("tauriTerminalPty", () => {
  it("attaches a shell tab by its index", async () => {
    await tauriTerminalPty.connect({ ptyKey: "s1:3", kind: "shell" }, () => {});
    expect(attachTab).toHaveBeenCalledWith("s1", 3, true, expect.anything());
  });

  it("reports a tab that ended instead of attaching", async () => {
    attachTab.mockResolvedValueOnce(false);
    const connecting = tauriTerminalPty.connect({ ptyKey: "s1:3", kind: "shell" }, () => {});
    await expect(connecting).rejects.toBeInstanceOf(TabEndedError);
    await expect(
      tauriTerminalPty.connect({ ptyKey: "s1:3", kind: "shell" }, () => {}),
    ).resolves.toBeUndefined();
  });
});
