import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../api", () => ({ editor: {}, pty: {}, sessions: {} }));
vi.mock("../terminal-views", () => ({ disposeTerminalView: vi.fn() }));

import type { Session } from "../types";
import {
  createLayout,
  focusedTabOf,
  leavesOf,
  serializeLayout,
  splitFocused,
  addTab,
  type Layout,
  type TabEntry,
} from "../layout-tree";
import {
  createTaskWorkspaceLayout,
  resolveRestoredLayoutSelection,
  workspaceOf,
  type TaskWorkspaceLayoutDeps,
  type WorkspaceAgent,
  type WorkspaceIdentity,
} from "../task-workspace-layout.svelte";

const TASK: WorkspaceIdentity = {
  kind: "task",
  key: "task:p1:PLA-1",
  projectId: "p1",
  taskKey: "PLA-1",
};
const OTHER: WorkspaceIdentity = {
  kind: "task",
  key: "task:p1:PLA-2",
  projectId: "p1",
  taskKey: "PLA-2",
};

function agent(sessionId: string): WorkspaceAgent {
  return { sessionId, label: sessionId, icon: "bot" };
}

function agentTab(ptyKey: string): TabEntry {
  return { ptyKey, label: ptyKey, icon: "bot", type: "agent" };
}

function shellTab(ptyKey: string): TabEntry {
  return { ptyKey, label: "Shell", icon: "terminal", type: "shell" };
}

function keys(layout: Layout | null): string[][] {
  return leavesOf(layout).map((leaf) => leaf.tabs.map((tab) => tab.ptyKey));
}

function setup(persisted: Record<string, Layout> = {}) {
  const saved = new Map<string, string>(
    Object.entries(persisted).map(([key, layout]) => [key, serializeLayout(layout)]),
  );
  const deps = {
    store: {
      load: vi.fn(async (workspace: WorkspaceIdentity) => saved.get(workspace.key) ?? null),
      save: vi.fn(async (workspace: WorkspaceIdentity, json: string) => {
        saved.set(workspace.key, json);
      }),
    },
    closeShell: vi.fn(async (_sessionId: string, _index: number) => {}),
    shellClosed: vi.fn(async (_ptyKey: string, _exited: boolean) => {}),
    handoffRestored: vi.fn((_ptyKey: string) => {}),
    getTerminalCommand: vi.fn(async (_sessionId: string, filePath: string) => `nvim ${filePath}`),
    disposeView: vi.fn(),
  } satisfies TaskWorkspaceLayoutDeps;
  const workspace = createTaskWorkspaceLayout(deps);
  const show = (target: WorkspaceIdentity, agents: string[], selected: string, explicit = true) =>
    workspace.show(target, {
      agents: agents.map(agent),
      selectedSessionId: selected,
      selectionIsExplicit: explicit,
    });
  return { workspace, deps, saved, show };
}

describe("showing a workspace", () => {
  it("builds a pane of agent tabs when nothing was persisted", async () => {
    const { workspace, show } = setup();
    expect(await show(TASK, ["a", "b"], "b")).toEqual({ adoptedSessionId: null, restored: false });
    expect(keys(workspace.layout)).toEqual([["a", "b"]]);
    expect(workspace.focusedTab()?.ptyKey).toBe("b");
    expect(workspace.workspace).toBe(TASK);
  });

  it("brings another agent of the loaded workspace forward without reloading", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a", "b"], "a");
    await show(TASK, ["a", "b"], "b");
    expect(workspace.focusedTab()?.ptyKey).toBe("b");
    expect(deps.store.load).toHaveBeenCalledOnce();
  });

  it("keeps focus on a clicked shell when selecting its session", async () => {
    // (a, b | b's shell): clicking the shell selects b, which must not pull
    // focus back to b's agent tab in the other pane.
    const { workspace, show } = setup();
    await show(TASK, ["a", "b"], "a");
    const shell = workspace.openShell("b", { split: "vertical" })!;
    await show(TASK, ["a", "b"], "b");
    expect(workspace.focusedTab()?.ptyKey).toBe(shell);
  });

  it("leaves the layout alone when the selection did not change", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a", "b"], "a");
    workspace.focusTab("b");
    const before = workspace.layout;
    await show(TASK, ["a", "b"], "a");
    expect(workspace.layout).toBe(before);
  });

  it("restores a persisted layout with its shells and splits", async () => {
    const split = splitFocused(createLayout([agentTab("a")]), "vertical")!;
    const persisted = addTab(split.layout, split.leafId, shellTab("a:2"));
    const { workspace, show } = setup({ [TASK.key]: persisted });
    expect(await show(TASK, ["a"], "a")).toEqual({ adoptedSessionId: null, restored: true });
    expect(keys(workspace.layout)).toEqual([["a"], ["a:2"]]);
  });

  it("adopts the remembered session when the selection was an arbitrary entry point", async () => {
    const { workspace, show } = setup({
      [TASK.key]: createLayout([agentTab("a"), agentTab("b")], "b"),
    });
    expect(await show(TASK, ["a", "b"], "a", false)).toEqual({
      adoptedSessionId: "b",
      restored: true,
    });
    expect(workspace.focusedTab()?.ptyKey).toBe("b");
  });

  it("honours an explicit pick over the remembered tab", async () => {
    const { workspace, show } = setup({
      [TASK.key]: createLayout([agentTab("a"), agentTab("b")], "b"),
    });
    expect((await show(TASK, ["a", "b"], "a", true))?.adoptedSessionId).toBeNull();
    expect(workspace.focusedTab()?.ptyKey).toBe("a");
  });

  it("never adopts a remembered session that is gone", async () => {
    const { workspace, show } = setup({
      [TASK.key]: createLayout([agentTab("a"), agentTab("gone")], "gone"),
    });
    expect((await show(TASK, ["a"], "a", false))?.adoptedSessionId).toBeNull();
    expect(keys(workspace.layout)).toEqual([["a"]]);
  });

  it("adds a tab for a selected session the layout does not know yet", async () => {
    const { workspace, show } = setup({ [TASK.key]: createLayout([agentTab("a")]) });
    await show(TASK, ["a", "new"], "new", true);
    expect(keys(workspace.layout)).toEqual([["a", "new"]]);
    expect(workspace.focusedTab()?.ptyKey).toBe("new");
  });

  it("saves the current layout before switching and ignores a superseded load", async () => {
    const { workspace, deps, saved, show } = setup();
    await show(TASK, ["a"], "a");
    workspace.openShell("a");
    let release!: () => void;
    deps.store.load.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve(null))),
    );
    const slow = show(OTHER, ["x"], "x");
    expect(JSON.parse(saved.get(TASK.key)!).tree.tabs).toHaveLength(2);
    const fast = show(TASK, ["a"], "a");
    release();
    expect(await slow).toBeNull();
    await fast;
    expect(workspace.workspace).toBe(TASK);
    expect(keys(workspace.layout)).toEqual([["a", "a:1"]]);
  });

  it("falls back to a fresh layout when the persisted one cannot be read", async () => {
    const { workspace, saved, show } = setup();
    saved.set(TASK.key, "{ not json");
    expect((await show(TASK, ["a"], "a"))?.restored).toBe(false);
    expect(keys(workspace.layout)).toEqual([["a"]]);
  });

  it("unloads on clear so the next show reloads", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    workspace.clear();
    expect(workspace.layout).toBeNull();
    await show(TASK, ["a"], "a");
    expect(deps.store.load).toHaveBeenCalledTimes(2);
  });
});

describe("reconciling sessions", () => {
  it("adds new agents and drops every tab of sessions that left", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a", "b"], "a");
    workspace.openShell("b");
    workspace.reconcile([agent("a"), agent("c")]);
    expect(keys(workspace.layout)).toEqual([["a", "c"]]);
  });

  it("does nothing while no workspace is loaded", () => {
    const { workspace } = setup();
    workspace.reconcile([agent("a")]);
    expect(workspace.layout).toBeNull();
  });
});

describe("shell tabs", () => {
  it("opens a shell in the focused pane and persists right away", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    deps.store.save.mockClear();
    expect(workspace.openShell("a")).toBe("a:1");
    expect(workspace.focusedTab()?.ptyKey).toBe("a:1");
    expect(deps.store.save).toHaveBeenCalledOnce();
  });

  it("never reuses an index held by a restored shell", async () => {
    // Shell 1 was closed before the restart; only shell 2 survives in the layout.
    const { workspace, show } = setup({
      [TASK.key]: createLayout([agentTab("a"), shellTab("a:2")]),
    });
    await show(TASK, ["a"], "a");
    expect(keys(workspace.layout)).toEqual([["a", "a:2"]]);
    expect(workspace.openShell("a")).toBe("a:3");
  });

  it("does not reuse a closed shell's index within a run", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    const first = workspace.openShell("a")!;
    await workspace.closeTab(first);
    expect(workspace.openShell("a")).toBe("a:2");
  });

  it("opens a shell in a new pane split off the focused one", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    expect(workspace.openShell("a", { split: "vertical" })).toBe("a:1");
    expect(keys(workspace.layout)).toEqual([["a"], ["a:1"]]);
    expect(workspace.isSplit).toBe(true);
    expect(workspace.focusedTab()?.ptyKey).toBe("a:1");
  });

  it("opens a shell in a given pane and focuses it", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    workspace.openShell("a", { split: "vertical" });
    const left = leavesOf(workspace.layout)[0].id;
    workspace.openShell("a", { paneId: left });
    expect(keys(workspace.layout)).toEqual([["a", "a:2"], ["a:1"]]);
    expect(workspace.focusedLeaf()?.id).toBe(left);
  });

  it("cannot open a shell without a layout", () => {
    const { workspace } = setup();
    expect(workspace.openShell("a")).toBeNull();
  });

  it("kills a shell on the backend before removing its tab", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    deps.closeShell.mockImplementationOnce(async () => {
      expect(workspace.findTab(ptyKey)).not.toBeNull();
    });
    expect(await workspace.closeTab(ptyKey)).toBe("closed");
    expect(deps.closeShell).toHaveBeenCalledWith("a", 1);
    expect(workspace.findTab(ptyKey)).toBeNull();
    expect(deps.disposeView).toHaveBeenCalledWith(ptyKey);
  });

  it("keeps a shell tab whose backend close failed", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    deps.closeShell.mockRejectedValueOnce(new Error("daemon refused"));
    await expect(workspace.closeTab(ptyKey)).rejects.toThrow("daemon refused");
    expect(workspace.findTab(ptyKey)).not.toBeNull();
    expect(deps.disposeView).not.toHaveBeenCalled();
  });

  it("removes a shell that exited by itself, then finalizes it on the backend", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    await workspace.shellExited(ptyKey);
    expect(workspace.findTab(ptyKey)).toBeNull();
    expect(deps.disposeView).toHaveBeenCalledWith(ptyKey);
    expect(deps.closeShell).toHaveBeenCalledWith("a", 1);
  });

  it("reports a self-exited shell as gone even when its backend close fails", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    deps.closeShell.mockRejectedValueOnce(new Error("daemon shell-tab kill timed out"));
    await expect(workspace.shellExited(ptyKey)).rejects.toThrow("timed out");
    expect(deps.shellClosed).toHaveBeenCalledWith(ptyKey, true);
    expect(workspace.findTab(ptyKey)).toBeNull();
  });

  it("reports an explicitly closed shell only once its backend confirms", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    deps.closeShell.mockRejectedValueOnce(new Error("shell may still be running"));
    await expect(workspace.closeTab(ptyKey)).rejects.toThrow("may still be running");
    expect(deps.shellClosed).not.toHaveBeenCalled();
    await workspace.closeTab(ptyKey);
    expect(deps.shellClosed).toHaveBeenCalledOnce();
    expect(deps.shellClosed).toHaveBeenCalledWith(ptyKey, false);
  });

  it("does not close again when the shell exits while its close is in flight", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    let finish!: () => void;
    deps.closeShell.mockReturnValueOnce(new Promise<void>((resolve) => (finish = resolve)));
    const closing = workspace.closeTab(ptyKey);
    // The kill ends the shell, whose exit arrives before the close returns.
    await workspace.shellExited(ptyKey);
    finish();
    await closing;
    expect(deps.closeShell).toHaveBeenCalledOnce();
    expect(deps.shellClosed).toHaveBeenCalledOnce();
    expect(workspace.findTab(ptyKey)).toBeNull();
  });

  it("makes a second close of a tab wait for the one in flight", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    let finish!: () => void;
    deps.closeShell.mockReturnValueOnce(new Promise<void>((resolve) => (finish = resolve)));
    const first = workspace.closeTab(ptyKey);
    const second = workspace.discardShell(ptyKey);
    // Nothing is reported closed before the backend confirms the first close.
    expect(deps.shellClosed).not.toHaveBeenCalled();
    finish();
    await Promise.all([first, second]);
    expect(deps.closeShell).toHaveBeenCalledOnce();
    expect(deps.shellClosed).toHaveBeenCalledOnce();
  });

  it("drops a shell that exited while its close failed", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    let fail!: (error: Error) => void;
    deps.closeShell.mockReturnValueOnce(new Promise((_, reject) => (fail = reject)));
    const closing = workspace.closeTab(ptyKey);
    await workspace.shellExited(ptyKey);
    fail(new Error("daemon shell-tab kill timed out"));
    await expect(closing).rejects.toThrow("timed out");
    expect(workspace.findTab(ptyKey)).toBeNull();
    expect(deps.shellClosed).toHaveBeenCalledWith(ptyKey, true);
  });

  it("does not close again when the exit follows an explicit close", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    await workspace.closeTab(ptyKey);
    deps.closeShell.mockClear();
    await workspace.shellExited(ptyKey);
    expect(deps.closeShell).not.toHaveBeenCalled();
  });

  it("releases a shell that failed to start", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    await workspace.shellFailedToStart(ptyKey);
    expect(deps.closeShell).toHaveBeenCalledWith("a", 1);
    expect(workspace.findTab(ptyKey)).toBeNull();
  });

  it("drops a shell that never started even when its backend close fails", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    deps.closeShell.mockRejectedValueOnce(new Error("daemon shell-tab kill timed out"));
    await expect(workspace.shellFailedToStart(ptyKey)).rejects.toThrow("timed out");
    // So a handoff terminal that never started still returns its session to the chat.
    expect(deps.shellClosed).toHaveBeenCalledWith(ptyKey, false);
    expect(workspace.findTab(ptyKey)).toBeNull();
  });
});

describe("across workspace switches", () => {
  it("opens nothing while another workspace is loading", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    let release!: () => void;
    deps.store.load.mockImplementationOnce(
      () => new Promise((resolve) => (release = () => resolve(null))),
    );
    const loading = show(OTHER, ["x"], "x");
    expect(workspace.openShell("x")).toBeNull();
    expect(workspace.openDiff("x")).toBe("unavailable");
    expect(workspace.openEditor("x", "a.ts")).toBe("unavailable");
    expect(await workspace.openTerminalEditor("x", "a.ts")).toBeNull();
    release();
    await loading;
    expect(keys(workspace.layout)).toEqual([["x"]]);
  });

  it("does not bring back a shell closed while its workspace was switched away", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    let killed!: () => void;
    deps.closeShell.mockImplementationOnce(
      () => new Promise<void>((resolve) => (killed = resolve)),
    );
    const closing = workspace.closeTab(ptyKey);
    await show(OTHER, ["x"], "x");
    killed();
    await closing;
    await show(TASK, ["a"], "a");
    expect(keys(workspace.layout)).toEqual([["a"]]);
  });

  it("finalizes and forgets a shell that exited in an unloaded workspace", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    await show(OTHER, ["x"], "x");
    await workspace.shellExited(ptyKey);
    expect(deps.closeShell).toHaveBeenCalledWith("a", 1);
    await show(TASK, ["a"], "a");
    expect(keys(workspace.layout)).toEqual([["a"]]);
  });

  it("builds a fresh layout when every remembered tab is gone", async () => {
    const { workspace, show } = setup({ [TASK.key]: createLayout([shellTab("a:1")]) });
    await show(OTHER, ["x"], "x");
    await workspace.shellExited("a:1");
    expect(await show(TASK, ["a"], "a")).toEqual({ adoptedSessionId: null, restored: false });
    expect(keys(workspace.layout)).toEqual([["a"]]);
  });
});

describe("workspaceOf", () => {
  const session = {
    id: "s1",
    project_id: "repo",
    task_key: null,
    task_project_id: null,
  } as Session;

  it("gives a task's sessions one workspace, keyed by the task's own project", () => {
    expect(workspaceOf({ ...session, task_key: "PLA-1", task_project_id: "owner" })).toEqual({
      kind: "task",
      key: "task:owner:PLA-1",
      projectId: "owner",
      taskKey: "PLA-1",
    });
  });

  it("puts every session linked to the same task in one workspace", () => {
    const other = {
      ...session,
      id: "s2",
      project_id: "elsewhere",
      task_key: "PLA-1",
      task_project_id: "owner",
    };
    expect(workspaceOf(other).key).toBe(
      workspaceOf({ ...session, task_key: "PLA-1", task_project_id: "owner" }).key,
    );
  });

  it("gives a session without a task a workspace of its own", () => {
    expect(workspaceOf(session)).toEqual({ kind: "session", key: "session:s1", sessionId: "s1" });
  });

  it("keys a workspace by identity, unaffected by session updates", () => {
    const renamed = { ...session, task_key: "PLA-1", name: "Renamed", status: "exited" } as Session;
    expect(workspaceOf(renamed).key).toBe(workspaceOf({ ...session, task_key: "PLA-1" }).key);
  });
});

describe("handoff terminals", () => {
  it("keeps a handoff terminal's mark across a reload, so its close still hands back", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openCommand("a", ["claude", "--resume"], "Terminal", {
      handoff: true,
    })!;
    expect(workspace.findTab(ptyKey)?.tab.handoff).toBe(true);
    expect(deps.handoffRestored).not.toHaveBeenCalled();

    // A webview reload or app restart loads the saved layout in a fresh instance.
    const reloaded = createTaskWorkspaceLayout(deps);
    await reloaded.show(TASK, {
      agents: [agent("a")],
      selectedSessionId: "a",
      selectionIsExplicit: true,
    });
    expect(deps.handoffRestored).toHaveBeenCalledWith(ptyKey);
  });

  it("adopts only the handoff terminals of sessions still in the workspace", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a", "b"], "a");
    workspace.openCommand("b", ["claude"], "Terminal", { handoff: true });
    const reloaded = createTaskWorkspaceLayout(deps);
    // b left the workspace (archived) while it was not shown.
    await reloaded.show(TASK, {
      agents: [agent("a")],
      selectedSessionId: "a",
      selectionIsExplicit: true,
    });
    expect(deps.handoffRestored).not.toHaveBeenCalled();
  });

  it("does not mark other command tabs", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openCommand("a", "htop", "htop")!;
    expect(workspace.findTab(ptyKey)?.tab.handoff).toBeUndefined();
  });
});

describe("discarding a shell", () => {
  it("closes it whatever its state, telling whether this layout held it", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openCommand("a", ["claude", "--resume"], "Terminal")!;
    // Still starting, which closeTab would refuse.
    expect(await workspace.closeTab(ptyKey)).toBe("starting");
    expect(await workspace.discardShell(ptyKey)).toBe(true);
    expect(workspace.findTab(ptyKey)).toBeNull();
    expect(deps.closeShell).toHaveBeenCalledWith("a", 1);
    expect(deps.shellClosed).toHaveBeenCalledWith(ptyKey, false);
    expect(await workspace.discardShell("other:3")).toBe(false);
  });
});

describe("terminal editor tabs", () => {
  it("reserves the editor command before the tab can mount", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = await workspace.openTerminalEditor("a", "src/main.rs");
    expect(ptyKey).toBe("a:1");
    expect(workspace.findTab(ptyKey!)?.tab.label).toBe("main.rs");
    expect(workspace.pendingCommand(ptyKey!)).toBe("nvim src/main.rs");
    expect(workspace.focusedTab()?.ptyKey).toBe(ptyKey);
  });

  it("consumes the command once the shell started", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = (await workspace.openTerminalEditor("a", "src/main.rs"))!;
    workspace.shellStarted(ptyKey);
    expect(workspace.pendingCommand(ptyKey)).toBeUndefined();
    expect(workspace.isStarting(ptyKey)).toBe(false);
  });

  it("refuses to close an editor tab that is still starting", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = (await workspace.openTerminalEditor("a", "src/main.rs"))!;
    expect(await workspace.closeTab(ptyKey)).toBe("starting");
    expect(deps.closeShell).not.toHaveBeenCalled();
  });

  it("opens nothing when the pane went away while the command resolved", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    let resolve!: (command: string) => void;
    deps.getTerminalCommand.mockImplementationOnce(() => new Promise((r) => (resolve = r)));
    const opening = workspace.openTerminalEditor("a", "src/main.rs");
    workspace.clear();
    resolve("nvim src/main.rs");
    expect(await opening).toBeNull();
    expect(workspace.layout).toBeNull();
  });

  it("releases the reservation when its shell fails to start", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = (await workspace.openTerminalEditor("a", "src/main.rs"))!;
    await workspace.shellFailedToStart(ptyKey);
    expect(workspace.isStarting(ptyKey)).toBe(false);
    expect(workspace.findTab(ptyKey)).toBeNull();
  });
});

describe("diff and editor resources", () => {
  it("opens the diff in the focused pane and focuses it when already open", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    expect(workspace.openDiff("a")).toBe("opened");
    workspace.focusTab("a");
    expect(workspace.openDiff("a")).toBe("focused");
    expect(workspace.focusedTab()?.ptyKey).toBe("a:diff");
    expect(workspace.openDiff("a")).toBe("focused");
  });

  it("toggles the diff closed only when it is in front", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    expect(workspace.toggleDiff("a")).toBe("opened");
    workspace.focusTab("a");
    expect(workspace.toggleDiff("a")).toBe("focused");
    expect(workspace.toggleDiff("a")).toBe("closed");
    expect(keys(workspace.layout)).toEqual([["a"]]);
  });

  it("keeps one embedded editor tab per file", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    expect(workspace.openEditor("a", "src/one.ts")).toBe("opened");
    expect(workspace.openEditor("a", "src/two.ts")).toBe("opened");
    expect(workspace.openEditor("a", "src/one.ts")).toBe("focused");
    expect(keys(workspace.layout)).toEqual([["a", "a:editor:src/one.ts", "a:editor:src/two.ts"]]);
    expect(workspace.findTab("a:editor:src/one.ts")?.tab.label).toBe("one.ts");
  });

  it("closes diff and editor tabs without the backend", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a"], "a");
    workspace.openDiff("a");
    expect(await workspace.closeTab("a:diff")).toBe("closed");
    expect(deps.closeShell).not.toHaveBeenCalled();
  });

  it("leaves agent tabs to the caller", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    expect(await workspace.closeTab("a")).toBe("agent");
    expect(keys(workspace.layout)).toEqual([["a"]]);
    expect(await workspace.closeTab("missing")).toBe("missing");
  });
});

describe("navigation", () => {
  it("cycles tabs and reports only real moves", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    expect(workspace.cycleTab(1)).toBeNull();
    workspace.openShell("a");
    expect(workspace.cycleTab(1)?.ptyKey).toBe("a");
    expect(workspace.cycleTab(-1)?.ptyKey).toBe("a:1");
  });

  it("titles a tab and keeps the title across reconciliation", async () => {
    const { workspace, show } = setup();
    await show(TASK, ["a"], "a");
    const ptyKey = workspace.openShell("a")!;
    workspace.setTabTitle(ptyKey, "cargo");
    workspace.reconcile([agent("a")]);
    expect(workspace.findTab(ptyKey)?.tab.label).toBe("cargo");
  });
});

describe("persistence timing", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("debounces saves of arrangement changes", async () => {
    const { workspace, deps, show } = setup();
    await show(TASK, ["a", "b"], "a");
    deps.store.save.mockClear();
    workspace.focusTab("b");
    workspace.focusTab("a");
    expect(deps.store.save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(500);
    expect(deps.store.save).toHaveBeenCalledOnce();
    expect(focusedTabOf(JSON.parse(deps.store.save.mock.calls[0][1]))?.ptyKey).toBe("a");
  });
});

describe("resolveRestoredLayoutSelection", () => {
  const base = {
    selectedSessionId: "a",
    restoredTreeHasTabForSelectedSession: true,
    selectionIsExplicit: false,
  };

  it("keeps the selection when the remembered tab matches it or is gone", () => {
    expect(resolveRestoredLayoutSelection({ ...base, liveRestoredSessionId: "a" })).toEqual({
      kind: "keep_selection",
    });
    expect(resolveRestoredLayoutSelection({ ...base, liveRestoredSessionId: null })).toEqual({
      kind: "focus_selected_session",
    });
  });

  it("never discards an explicit selection that has no tab yet", () => {
    expect(
      resolveRestoredLayoutSelection({
        ...base,
        liveRestoredSessionId: "b",
        selectionIsExplicit: true,
        restoredTreeHasTabForSelectedSession: false,
      }),
    ).toEqual({ kind: "keep_selection" });
  });

  it("only adopts a live, remembered, non-explicit selection", () => {
    expect(resolveRestoredLayoutSelection({ ...base, liveRestoredSessionId: "b" })).toEqual({
      kind: "adopt_restored_session",
      sessionId: "b",
    });
    expect(
      resolveRestoredLayoutSelection({
        ...base,
        liveRestoredSessionId: "b",
        selectionIsExplicit: true,
      }),
    ).toEqual({ kind: "focus_selected_session" });
  });
});
