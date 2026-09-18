import { describe, expect, it, vi } from "vitest";
import { queueTerminalEditor } from "../terminal-editor";

describe("queueTerminalEditor", () => {
  it("persists a shell tab before adding it to the split tree and queues its command", async () => {
    const events: string[] = [];
    const pending = new Map<string, string>();

    const opened = await queueTerminalEditor({
      sessionId: "session-1",
      filePath: "src/main.rs",
      getTerminalCommand: async () => {
        events.push("command");
        return "nvim src/main.rs";
      },
      getFocusedLeafId: () => "leaf-1",
      addTab: () => {
        events.push("tab");
        return 2;
      },
      removeTab: vi.fn(),
      incrementTabCount: async () => {
        events.push("persist");
      },
      addShellTab: (leafId, ptyKey, label) => {
        events.push(`tree:${leafId}:${ptyKey}:${label}`);
      },
      pendingCommands: pending,
    });

    expect(opened).toBe(true);
    expect(events).toEqual(["command", "tab", "persist", "tree:leaf-1:session-1:2:main.rs"]);
    expect(pending.get("session-1:2")).toBe("nvim src/main.rs");
  });

  it("rolls back local tab state and does not mount a terminal when persistence fails", async () => {
    const removeTab = vi.fn();
    const addShellTab = vi.fn();
    const pending = new Map<string, string>();

    await expect(
      queueTerminalEditor({
        sessionId: "session-1",
        filePath: "src/main.rs",
        getTerminalCommand: async () => "nvim src/main.rs",
        getFocusedLeafId: () => "leaf-1",
        addTab: () => 2,
        removeTab,
        incrementTabCount: async () => {
          throw new Error("database unavailable");
        },
        addShellTab,
        pendingCommands: pending,
      }),
    ).rejects.toThrow("database unavailable");

    expect(removeTab).toHaveBeenCalledWith("session-1", 2);
    expect(addShellTab).not.toHaveBeenCalled();
    expect(pending.size).toBe(0);
  });

  it("does nothing when no split leaf is focused", async () => {
    const getTerminalCommand = vi.fn();
    const addTab = vi.fn();

    await expect(
      queueTerminalEditor({
        sessionId: "session-1",
        filePath: "src/main.rs",
        getTerminalCommand,
        getFocusedLeafId: () => null,
        addTab,
        removeTab: vi.fn(),
        incrementTabCount: vi.fn(),
        addShellTab: vi.fn(),
        pendingCommands: new Map(),
      }),
    ).resolves.toBe(false);

    expect(getTerminalCommand).not.toHaveBeenCalled();
    expect(addTab).not.toHaveBeenCalled();
  });
});

describe("rollbackPendingTerminalEditor", () => {
  it("removes a failed terminal editor tab and decrements its persisted count", async () => {
    const { rollbackPendingTerminalEditor } = await import("../terminal-editor");
    const pending = new Map([["session-1:2", "nvim src/main.rs"]]);
    const removeTab = vi.fn();
    const closeTab = vi.fn(() => Promise.resolve());

    await expect(
      rollbackPendingTerminalEditor({
        ptyKey: "session-1:2",
        pendingCommands: pending,
        removeTab,
        closeTab,
      }),
    ).resolves.toBe(true);

    expect(pending.size).toBe(0);
    expect(removeTab).toHaveBeenCalledWith("session-1", 2);
    expect(closeTab).toHaveBeenCalledWith("session-1", 2);
  });

  it("does not affect ordinary shell tabs", async () => {
    const { rollbackPendingTerminalEditor } = await import("../terminal-editor");
    const removeTab = vi.fn();
    const closeTab = vi.fn();

    await expect(
      rollbackPendingTerminalEditor({
        ptyKey: "session-1:2",
        pendingCommands: new Map(),
        removeTab,
        closeTab,
      }),
    ).resolves.toBe(false);

    expect(removeTab).not.toHaveBeenCalled();
    expect(closeTab).not.toHaveBeenCalled();
  });
});

describe("pending terminal editor commands", () => {
  it("makes a queued command available for shell startup and consumes it after spawn", async () => {
    const { consumePendingTerminalEditorCommand, getPendingTerminalEditorCommand } =
      await import("../terminal-editor");
    const pending = new Map([["session-1:2", "nvim src/main.rs"]]);
    const command = { ptyKey: "session-1:2", pendingCommands: pending };

    expect(getPendingTerminalEditorCommand(command)).toBe("nvim src/main.rs");
    expect(consumePendingTerminalEditorCommand(command)).toBe(true);
    expect(getPendingTerminalEditorCommand(command)).toBeUndefined();
  });
});
