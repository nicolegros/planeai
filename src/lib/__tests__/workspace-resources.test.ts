import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api", () => ({
  pty: { incrementTabCount: vi.fn(() => Promise.resolve()) },
}));

import { pty } from "../api";
import { destroySession, getTabs, initSession } from "../session-tabs.svelte";
import * as splitTree from "../split-tree.svelte";
import {
  openDiffResource,
  openEditorResource,
  openShellResource,
  openShellResourceInFocusedLeaf,
  openShellResourceInNewPane,
  toggleDiffResource,
} from "../workspace-resources";

const SESSION = "agent-1";

function agentTab() {
  return { ptyKey: SESSION, label: "Agent", icon: "bot", type: "agent" as const };
}

function ptyKeys(leafId?: string): string[] {
  const leaf = leafId ? splitTree.getLeafById(leafId) : splitTree.getFocusedLeaf();
  return leaf?.tabs.map((tab) => tab.ptyKey) ?? [];
}

beforeEach(() => {
  vi.clearAllMocks();
  splitTree.resetTree();
  destroySession(SESSION);
  initSession(SESSION, 1);
  splitTree.initTree([agentTab()], SESSION);
});

describe("openShellResourceInFocusedLeaf", () => {
  it("adds a shell tab and persists the session's tab count", () => {
    const ptyKey = openShellResourceInFocusedLeaf(SESSION);

    expect(ptyKey).toBe(`${SESSION}:1`);
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:1`]);
    expect(splitTree.getFocusedLeaf()?.activeTab).toBe(`${SESSION}:1`);
    expect(pty.incrementTabCount).toHaveBeenCalledWith(SESSION);
  });

  it("gives each shell tab its own pty key", () => {
    openShellResourceInFocusedLeaf(SESSION);
    openShellResourceInFocusedLeaf(SESSION);

    const keys = ptyKeys();
    expect(keys).toEqual([SESSION, `${SESSION}:1`, `${SESSION}:2`]);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("does nothing when the session has no tab state", () => {
    destroySession(SESSION);

    expect(openShellResourceInFocusedLeaf(SESSION)).toBeNull();
    expect(ptyKeys()).toEqual([SESSION]);
    expect(pty.incrementTabCount).not.toHaveBeenCalled();
  });
});

describe("openShellResource", () => {
  it("returns the session tab index and skips persistence when the pty key is taken", () => {
    // Desynchronise the tree from the tab counter: the next index is already used.
    splitTree.addSessionToLeaf(splitTree.getFocusedLeafId()!, {
      ptyKey: `${SESSION}:1`,
      label: "Shell",
      icon: "terminal",
      type: "shell",
    });

    expect(openShellResource(SESSION, splitTree.getFocusedLeafId()!)).toBeNull();
    expect(getTabs(SESSION).map((tab) => tab.index)).toEqual([0]);
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:1`]);
    expect(pty.incrementTabCount).not.toHaveBeenCalled();
  });
});

describe("openShellResourceInNewPane", () => {
  it("puts the shell tab in the new pane", () => {
    const ptyKey = openShellResourceInNewPane(SESSION, "vertical");

    expect(ptyKey).toBe(`${SESSION}:1`);
    expect(splitTree.getTree()?.type).toBe("split");
    expect(splitTree.getLeafForSession(`${SESSION}:1`)!.id).not.toBe(
      splitTree.getLeafForSession(SESSION)!.id,
    );
  });

  it("undoes the split when the shell tab cannot be created", () => {
    destroySession(SESSION);

    expect(openShellResourceInNewPane(SESSION, "vertical")).toBeNull();
    expect(splitTree.getTree()?.type).toBe("leaf");
    expect(ptyKeys()).toEqual([SESSION]);
  });
});

describe("openDiffResource", () => {
  it("opens the diff in the focused leaf", () => {
    expect(openDiffResource(SESSION)).toBe("opened");
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:diff`]);
  });

  it("focuses an already-open diff rather than closing it", () => {
    openDiffResource(SESSION);
    splitTree.focusTab(SESSION);

    expect(openDiffResource(SESSION)).toBe("focused");
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:diff`]);
    expect(splitTree.getFocusedLeaf()?.activeTab).toBe(`${SESSION}:diff`);
  });

  it("is idempotent when the diff is already the active tab", () => {
    openDiffResource(SESSION);

    expect(openDiffResource(SESSION)).toBe("focused");
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:diff`]);
    expect(splitTree.getFocusedLeaf()?.activeTab).toBe(`${SESSION}:diff`);
  });
});

describe("toggleDiffResource", () => {
  it("opens the diff in the focused leaf", () => {
    expect(toggleDiffResource(SESSION)).toBe("opened");
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:diff`]);
  });

  it("closes the diff when it is already active", () => {
    toggleDiffResource(SESSION);

    expect(toggleDiffResource(SESSION)).toBe("closed");
    expect(ptyKeys()).toEqual([SESSION]);
  });

  it("focuses an open but inactive diff instead of opening a second one", () => {
    toggleDiffResource(SESSION);
    splitTree.focusTab(SESSION);

    expect(toggleDiffResource(SESSION)).toBe("focused");
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:diff`]);
    expect(splitTree.getFocusedLeaf()?.activeTab).toBe(`${SESSION}:diff`);
  });
});

describe("openEditorResource", () => {
  it("opens a file with its name as the tab label", () => {
    expect(openEditorResource(SESSION, "src/lib/main.rs")).toBe("opened");

    const tab = splitTree.findTab(`${SESSION}:editor:src/lib/main.rs`)!.tab;
    expect(tab.label).toBe("main.rs");
    expect(tab.filePath).toBe("src/lib/main.rs");
  });

  it("focuses the existing tab when the same file is opened twice", () => {
    openEditorResource(SESSION, "src/lib/main.rs");
    splitTree.focusTab(SESSION);

    expect(openEditorResource(SESSION, "src/lib/main.rs")).toBe("focused");
    expect(ptyKeys()).toEqual([SESSION, `${SESSION}:editor:src/lib/main.rs`]);
  });

  it("keeps separate tabs for different files", () => {
    openEditorResource(SESSION, "src/a.rs");
    openEditorResource(SESSION, "src/b.rs");

    const keys = ptyKeys();
    expect(keys).toHaveLength(3);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
