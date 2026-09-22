import { beforeEach, describe, expect, it } from "vitest";
import { mount, unmount } from "svelte";
import TabStrip from "../../components/TabStrip.svelte";
import * as splitTree from "../split-tree.svelte";
import type { TabEntry } from "../split-tree.svelte";
import { addTab, getTabs, initSession } from "../session-tabs.svelte";
import { removeTab } from "../session-tabs.svelte";
import { queueTerminalEditor } from "../terminal-editor";
import { reconcileWorkspaceTabs } from "../workspace-tabs";

const SESSION = "agent-1";
const FILE = "/repo/src/main.rs";
const COMMAND = `nvim ${FILE}`;

/** Mirror App.svelte's buildTabEntriesForWorkspace for a single-agent workspace. */
function workspaceEntries(): TabEntry[] {
  return getTabs(SESSION).map((tab) => ({
    ptyKey: tab.index === 0 ? SESSION : `${SESSION}:${tab.index}`,
    label: tab.index === 0 ? "Agent" : "Agent · Shell",
    icon: "terminal",
    type: tab.index === 0 ? ("agent" as const) : ("shell" as const),
  }));
}

function reconcile(reservedPtyKeys: Map<string, string>): void {
  reconcileWorkspaceTabs({
    workspaceEntries: workspaceEntries(),
    validSessionIds: new Set([SESSION]),
    reservedPtyKeys,
    tree: splitTree,
  });
}

/**
 * Open a file with the shell editor while the workspace reconcile effect fires
 * during the in-flight tab-count persistence.
 */
async function openShellEditor(): Promise<{
  pending: Map<string, string>;
  reservedAtReconcile: boolean;
}> {
  const pending = new Map<string, string>();
  let reservedAtReconcile = false;
  await queueTerminalEditor({
    sessionId: SESSION,
    filePath: FILE,
    getTerminalCommand: async () => COMMAND,
    getFocusedLeafId: splitTree.getFocusedLeafId,
    addTab,
    removeTab,
    incrementTabCount: async () => {
      reservedAtReconcile = pending.has(`${SESSION}:1`);
      reconcile(pending);
    },
    addShellTab: (leafId, ptyKey, label) => {
      splitTree.addSessionToLeaf(leafId, { ptyKey, label, icon: "terminal", type: "shell" });
    },
    pendingCommands: pending,
  });
  return { pending, reservedAtReconcile };
}

describe("shell editor tabs under workspace reconciliation", () => {
  beforeEach(() => {
    splitTree.resetTree();
    initSession(SESSION, 1);
    splitTree.initTree(workspaceEntries(), SESSION);
  });

  it("adds exactly one tab per pty key so the keyed tab render cannot crash", async () => {
    await openShellEditor();

    const leaf = splitTree.getFocusedLeaf();
    const ptyKeys = leaf?.tabs.map((tab) => tab.ptyKey) ?? [];
    expect(ptyKeys).toEqual([SESSION, `${SESSION}:1`]);
    expect(new Set(ptyKeys).size).toBe(ptyKeys.length);
  });

  it("reserves the editor pty key before reconciliation can mount a bare shell", async () => {
    const { pending, reservedAtReconcile } = await openShellEditor();

    expect(reservedAtReconcile).toBe(true);
    expect(pending.get(`${SESSION}:1`)).toBe(COMMAND);
  });

  it("labels the editor tab with the file name and makes it active", async () => {
    await openShellEditor();

    const leaf = splitTree.getFocusedLeaf();
    expect(leaf?.activeTab).toBe(`${SESSION}:1`);
    expect(leaf?.tabs.find((tab) => tab.ptyKey === `${SESSION}:1`)?.label).toBe("main.rs");
  });

  it("renders the resulting tab strip instead of crashing on a duplicate key", async () => {
    await openShellEditor();

    const leaf = splitTree.getFocusedLeaf()!;
    // Mirror App.svelte's getLeafTabInfo: tabs are keyed by pty key.
    const tabs = leaf.tabs.map((tab, visualIndex) => ({
      id: tab.ptyKey,
      index: tab.type === "agent" ? 0 : visualIndex,
      label: tab.label,
      icon: tab.icon,
    }));

    const target = document.body.appendChild(document.createElement("div"));
    const component = mount(TabStrip, {
      target,
      props: { tabs, activeTabIndex: 1, activeTabId: leaf.activeTab, onSelectTab: () => {} },
    });
    expect(target.querySelectorAll("[role='tab']")).toHaveLength(2);
    expect(target.textContent).toContain("main.rs");
    unmount(component);
    target.remove();
  });

  it("drops the reservation and the session tab when persisting the count fails", async () => {
    const pending = new Map<string, string>();
    await expect(
      queueTerminalEditor({
        sessionId: SESSION,
        filePath: FILE,
        getTerminalCommand: async () => COMMAND,
        getFocusedLeafId: splitTree.getFocusedLeafId,
        addTab,
        removeTab,
        incrementTabCount: async () => {
          throw new Error("daemon offline");
        },
        addShellTab: () => {
          throw new Error("must not mount a shell tab");
        },
        pendingCommands: pending,
      }),
    ).rejects.toThrow("daemon offline");

    expect(pending.size).toBe(0);
    expect(getTabs(SESSION).map((tab) => tab.index)).toEqual([0]);
    reconcile(pending);
    expect(splitTree.getFocusedLeaf()?.tabs.map((tab) => tab.ptyKey)).toEqual([SESSION]);
  });
});
