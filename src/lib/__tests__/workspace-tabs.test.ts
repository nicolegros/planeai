import { beforeEach, describe, expect, it } from "vitest";
import * as splitTree from "../split-tree.svelte";
import type { TabEntry } from "../split-tree.svelte";
import { ptyKeyToSessionId, reconcileWorkspaceTabs } from "../workspace-tabs";

function agent(sessionId: string): TabEntry {
  return { ptyKey: sessionId, label: sessionId, icon: "bot", type: "agent" };
}

function shell(ptyKey: string): TabEntry {
  return { ptyKey, label: "Shell", icon: "terminal", type: "shell" };
}

describe("ptyKeyToSessionId", () => {
  it("strips shell, diff and editor suffixes", () => {
    expect(ptyKeyToSessionId("agent-1")).toBe("agent-1");
    expect(ptyKeyToSessionId("agent-1:2")).toBe("agent-1");
    expect(ptyKeyToSessionId("agent-1:diff")).toBe("agent-1");
    expect(ptyKeyToSessionId("agent-1:editor:src/main.rs")).toBe("agent-1");
  });
});

describe("reconcileWorkspaceTabs", () => {
  beforeEach(() => {
    splitTree.resetTree();
  });

  it("adds restored task-session tabs without stealing the active tab", () => {
    splitTree.initTree([agent("agent-1")], "agent-1");

    reconcileWorkspaceTabs({
      workspaceEntries: [agent("agent-1"), agent("agent-2")],
      validSessionIds: new Set(["agent-1", "agent-2"]),
      tree: splitTree,
    });

    const leaf = splitTree.getFocusedLeaf();
    expect(leaf?.tabs.map((tab) => tab.ptyKey)).toEqual(["agent-1", "agent-2"]);
    expect(leaf?.activeTab).toBe("agent-1");
  });

  it("prunes tabs whose session left the workspace", () => {
    splitTree.initTree([agent("agent-1"), agent("agent-2"), shell("agent-2:1")], "agent-1");

    reconcileWorkspaceTabs({
      workspaceEntries: [agent("agent-1")],
      validSessionIds: new Set(["agent-1"]),
      tree: splitTree,
    });

    expect(splitTree.getFocusedLeaf()?.tabs.map((tab) => tab.ptyKey)).toEqual(["agent-1"]);
  });

  it("leaves reserved pty keys for the flow that is still creating them", () => {
    splitTree.initTree([agent("agent-1")], "agent-1");

    reconcileWorkspaceTabs({
      workspaceEntries: [agent("agent-1"), shell("agent-1:1")],
      validSessionIds: new Set(["agent-1"]),
      reservedPtyKeys: new Map([["agent-1:1", "nvim src/main.rs"]]),
      tree: splitTree,
    });

    expect(splitTree.getFocusedLeaf()?.tabs.map((tab) => tab.ptyKey)).toEqual(["agent-1"]);
  });

  it("relabels tabs when their session is renamed", () => {
    splitTree.initTree([agent("agent-1"), shell("agent-1:1"), agent("agent-2")], "agent-1");

    reconcileWorkspaceTabs({
      workspaceEntries: [
        { ...agent("agent-1"), label: "Fix login (2)" },
        { ...shell("agent-1:1"), label: "Fix login (2) · Shell" },
        agent("agent-2"),
      ],
      validSessionIds: new Set(["agent-1", "agent-2"]),
      tree: splitTree,
    });

    expect(splitTree.getFocusedLeaf()?.tabs.map((tab) => tab.label)).toEqual([
      "Fix login (2)",
      "Fix login (2) · Shell",
      "agent-2",
    ]);
    expect(splitTree.getFocusedLeaf()?.tabs.some((tab) => tab.customTitle)).toBe(false);
  });

  it("keeps a title the shell set for itself", () => {
    splitTree.initTree(
      [agent("agent-1"), { ...shell("agent-1:1"), label: "nvim", customTitle: true }],
      "agent-1",
    );

    reconcileWorkspaceTabs({
      workspaceEntries: [agent("agent-1"), { ...shell("agent-1:1"), label: "Renamed · Shell" }],
      validSessionIds: new Set(["agent-1"]),
      tree: splitTree,
    });

    expect(splitTree.getFocusedLeaf()?.tabs[1]?.label).toBe("nvim");
  });
});
