/**
 * TaskWorkspace layout: the tabs and splits of the loaded TaskWorkspace.
 *
 * Owns the layout tree (see ./layout-tree), which workspace it belongs to, its
 * persistence, shell tab index allocation, terminal-editor command reservations,
 * and closing tabs against the backend. The persisted layout is the authority on
 * which shell tabs exist: there is no separate per-session tab record.
 *
 * Callers own the UI follow-up (focus, snackbars, session selection); every
 * operation reports what happened so they can react.
 */
import * as tree from "./layout-tree";
import type { Layout, LeafNode, NavDirection, SplitDirection, TabEntry } from "./layout-tree";
import {
  agentPtyKey,
  diffPtyKey,
  editorPtyKey,
  parsePtyKey,
  ptyKeySessionId,
  shellPtyKey,
} from "./pty-key";
import { editor as editorApi, providerSessions, pty, sessions as sessionsApi } from "./api";
import { toTaskWorkspaceId } from "./sidebar-session-order";
import { sessionTaskProjectId, type Session } from "./types";
import { disposeTerminalView } from "./terminal-views";
import { createProviderHandoff } from "./provider-handoff";
import { showSnackbar } from "./snackbar.svelte";
import type { TabCommand } from "./terminal-view";

export type WorkspaceIdentity =
  | { kind: "task"; key: string; projectId: string; taskKey: string }
  | { kind: "session"; key: string; sessionId: string };

/** The workspace a session belongs to: its task's, or its own when it has no task. */
export function workspaceOf(session: Session): WorkspaceIdentity {
  const projectId = sessionTaskProjectId(session);
  return session.task_key
    ? {
        kind: "task",
        key: toTaskWorkspaceId(projectId, session.task_key),
        projectId,
        taskKey: session.task_key,
      }
    : { kind: "session", key: `session:${session.id}`, sessionId: session.id };
}

/** An agent session of the workspace, as its tab shows it. */
export interface WorkspaceAgent {
  sessionId: string;
  label: string;
  icon: string;
}

export interface LayoutStore {
  load: (workspace: WorkspaceIdentity) => Promise<string | null>;
  save: (workspace: WorkspaceIdentity, layoutJson: string) => Promise<unknown>;
}

export interface TaskWorkspaceLayoutDeps {
  store: LayoutStore;
  /** Kill a shell tab's backend process. Rejects when it may still be running. */
  closeShell: (sessionId: string, index: number) => Promise<unknown>;
  /** A shell tab left the layout, closed or exited by itself; it never rejects. */
  /** `exited` when its process ended by itself rather than being closed. */
  shellClosed?: (ptyKey: string, exited: boolean) => Promise<void>;
  getTerminalCommand: (sessionId: string, filePath: string) => Promise<string>;
  disposeView: (ptyKey: string) => void;
  saveDelayMs?: number;
  onSaveError?: (workspace: WorkspaceIdentity, error: unknown) => void;
}

export interface ShowOptions {
  agents: WorkspaceAgent[];
  selectedSessionId: string;
  /**
   * A user choice, rather than an arbitrary entry point: selecting a task passes
   * its *first* session purely so there is something to load, and treating that
   * as a choice would always land on the task's first session.
   */
  selectionIsExplicit: boolean;
}

export interface ShowResult {
  /** Session the restored layout wants selected instead of the requested one. */
  adoptedSessionId: string | null;
  /** A persisted layout was restored, so its front terminal should take focus. */
  restored: boolean;
}

/** Outcome of opening a resource that may already be in the layout. */
export type ResourceOpen = "opened" | "focused" | "closed" | "unavailable";

/**
 * Outcome of closing a tab. `agent` tabs are durable sessions the caller parks;
 * `starting` is a terminal editor whose shell has not attached yet.
 */
export type TabClose = "closed" | "agent" | "starting" | "missing";

/** A tab as a tab strip renders it; `id` is its pty key. */
export interface PaneTab {
  id: string;
  label: string;
  /** Muted display-only suffix, never persisted with the label. */
  detail?: string;
  icon: string;
}

export type TaskWorkspaceLayout = ReturnType<typeof createTaskWorkspaceLayout>;

export function createTaskWorkspaceLayout(deps: TaskWorkspaceLayoutDeps) {
  const saveDelayMs = deps.saveDelayMs ?? 500;

  let layout = $state.raw<Layout | null>(null);
  let workspace = $state.raw<WorkspaceIdentity | null>(null);
  let loading = $state(false);
  let agents: WorkspaceAgent[] = [];
  /** Session the workspace is focused on; its agent tab is preferred on reconcile. */
  let focusedSessionId: string | null = null;
  let generation = 0;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** Editor commands for shell tabs that have not attached yet, by pty key. */
  const pendingCommands = new Map<string, TabCommand>();
  /**
   * Highest shell index handed out per session in this app run. Indices are not
   * reused within a run, so a late `pty-exited` for a closed shell can never hit
   * a new tab that took its key.
   */
  const highestShellIndex = new Map<string, number>();
  /**
   * Shells closed or exited in this run. A layout saved while one was still open
   * (another workspace was loaded when it went) must not bring it back.
   */
  const goneShells = new Set<string>();

  function commit(next: Layout | null): void {
    if (next === layout) return;
    layout = next;
    scheduleSave();
  }

  function scheduleSave(): void {
    if (!workspace || loading) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(saveNow, saveDelayMs);
  }

  /** Persist right away; shell tabs must be durable before their process can outlive the app. */
  function saveNow(): void {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
    const target = workspace;
    if (!layout || !target || loading) return;
    deps.store
      .save(target, tree.serializeLayout(layout))
      .catch((error) => deps.onSaveError?.(target, error));
  }

  function agentTabs(): TabEntry[] {
    return agents.map((agent) => ({
      ptyKey: agentPtyKey(agent.sessionId),
      label: agent.label,
      icon: agent.icon,
      type: "agent",
    }));
  }

  function reconcileNow(): void {
    commit(
      tree.reconcile(layout, {
        agentTabs: agentTabs(),
        preferredActiveTab: focusedSessionId ? agentPtyKey(focusedSessionId) : undefined,
      }),
    );
  }

  function allocateShellIndex(sessionId: string): number {
    let highest = highestShellIndex.get(sessionId) ?? 0;
    const keys = [...tree.tabsOf(layout).map((tab) => tab.ptyKey), ...pendingCommands.keys()];
    for (const key of keys) {
      const parts = parsePtyKey(key);
      if (parts?.kind === "shell" && parts.sessionId === sessionId)
        highest = Math.max(highest, parts.index);
    }
    highestShellIndex.set(sessionId, highest + 1);
    return highest + 1;
  }

  /** Reserve a shell tab whose PTY runs `command`, so the terminal that mounts for it starts it. */
  function openCommandTab(
    sessionId: string,
    paneId: string,
    command: TabCommand,
    label: string,
    focus = false,
  ): string | null {
    if (!layout || loading || !tree.findLeaf(layout, paneId)) return null;
    const ptyKey = shellPtyKey(sessionId, allocateShellIndex(sessionId));
    pendingCommands.set(ptyKey, command);
    const next = tree.addTab(layout, paneId, shellTab(ptyKey, label));
    commit(focus ? tree.focusLeaf(next, paneId) : next);
    saveNow();
    return ptyKey;
  }

  function shellTab(ptyKey: string, label: string): TabEntry {
    return { ptyKey, label, icon: "terminal", type: "shell" };
  }

  function removeShell(ptyKey: string): void {
    goneShells.add(ptyKey);
    if (layout) commit(tree.removeTab(layout, ptyKey));
    deps.disposeView(ptyKey);
    saveNow();
  }

  function openDiff(sessionId: string): ResourceOpen {
    if (!layout || loading) return "unavailable";
    const ptyKey = diffPtyKey(sessionId);
    if (tree.findTab(layout, ptyKey)) {
      commit(tree.focusTab(layout, ptyKey));
      return "focused";
    }
    commit(
      tree.addTab(layout, layout.focusedLeafId, {
        ptyKey,
        label: "Diff",
        icon: "git-compare",
        type: "diff",
      }),
    );
    return "opened";
  }

  function pruneGoneShells(restored: Layout): Layout | null {
    let result: Layout | null = restored;
    for (const ptyKey of goneShells) if (result) result = tree.removeTab(result, ptyKey);
    return result;
  }

  /**
   * Removes a shell whose process is gone or never started; the backend close after it
   * only finalizes it, and may reject.
   */
  async function dropShell(ptyKey: string, exited: boolean): Promise<void> {
    const parts = parsePtyKey(ptyKey);
    // An explicit close already finalized it; closing again would be redundant.
    if (parts?.kind !== "shell" || goneShells.has(ptyKey)) return;
    pendingCommands.delete(ptyKey);
    removeShell(ptyKey);
    await deps.shellClosed?.(ptyKey, exited);
    await deps.closeShell(parts.sessionId, parts.index);
  }

  async function closeShell(ptyKey: string): Promise<void> {
    const parts = parsePtyKey(ptyKey);
    if (parts?.kind !== "shell") return;
    // Keep the tab until the backend confirms, so a failed kill does not leave a
    // shell running with no UI to reach it.
    await deps.closeShell(parts.sessionId, parts.index);
    removeShell(ptyKey);
    await deps.shellClosed?.(ptyKey, false);
  }

  return {
    get layout(): Layout | null {
      return layout;
    },
    get workspace(): WorkspaceIdentity | null {
      return workspace;
    },
    /** A workspace layout is being fetched; the shown layout still belongs to the previous one. */
    get loading(): boolean {
      return loading;
    },
    get isSplit(): boolean {
      return tree.isSplit(layout);
    },
    focusedLeaf: (): LeafNode | null => tree.focusedLeafOf(layout),
    focusedTab: (): TabEntry | null => tree.focusedTabOf(layout),
    findTab: (ptyKey: string) => tree.findTab(layout, ptyKey),

    // ─── Lifecycle ────────────────────────────────────────────────────────────

    /**
     * Show a workspace focused on a session. Within the loaded workspace this
     * only brings that session's tab forward; another workspace saves the
     * current layout and restores its own, or builds one from its agents.
     *
     * Resolves null when a newer `show` or `clear` superseded this one.
     */
    async show(target: WorkspaceIdentity, options: ShowOptions): Promise<ShowResult | null> {
      const selectedTab = agentPtyKey(options.selectedSessionId);
      if (workspace?.key === target.key && !loading) {
        agents = options.agents;
        if (options.selectedSessionId !== focusedSessionId) {
          focusedSessionId = options.selectedSessionId;
          // Selecting a session from one of its shells keeps that shell focused.
          const front = tree.focusedTabOf(layout);
          const frontIsSelected =
            !!front && ptyKeySessionId(front.ptyKey) === options.selectedSessionId;
          const existing = tree.findTab(layout, selectedTab);
          if (layout && existing && !frontIsSelected && existing.leaf.activeTab !== selectedTab) {
            commit(tree.focusTab(layout, selectedTab));
          }
        }
        reconcileNow();
        return { adoptedSessionId: null, restored: false };
      }

      saveNow();
      const loadGeneration = ++generation;
      loading = true;
      let restored: Layout | null = null;
      try {
        const json = await deps.store.load(target);
        restored = json ? tree.restoreLayout(json) : null;
      } catch (error) {
        console.warn("Failed to load workspace layout", target.key, error);
      }
      if (loadGeneration !== generation) return null;

      agents = options.agents;
      let adoptedSessionId: string | null = null;
      if (restored) {
        const restoredTab = tree.focusedTabOf(restored);
        const restoredSessionId = restoredTab ? ptyKeySessionId(restoredTab.ptyKey) : null;
        const selection = resolveRestoredLayoutSelection({
          liveRestoredSessionId:
            restoredSessionId && agents.some((agent) => agent.sessionId === restoredSessionId)
              ? restoredSessionId
              : null,
          selectedSessionId: options.selectedSessionId,
          restoredTreeHasTabForSelectedSession: !!tree.findTab(restored, selectedTab),
          selectionIsExplicit: options.selectionIsExplicit,
        });
        if (selection.kind === "focus_selected_session") {
          restored = tree.focusTab(restored, selectedTab);
        }
        if (selection.kind === "adopt_restored_session") adoptedSessionId = selection.sessionId;
      }
      const pruned = restored && pruneGoneShells(restored);
      layout = pruned || tree.createLayout(agentTabs(), selectedTab);
      workspace = target;
      focusedSessionId = adoptedSessionId ?? options.selectedSessionId;
      loading = false;
      reconcileNow();
      if (pruned !== restored) saveNow();
      return { adoptedSessionId, restored: !!pruned };
    },

    /** Re-sync tabs with the workspace's agents (sessions added, renamed, or gone). */
    reconcile(nextAgents: WorkspaceAgent[]): void {
      if (!workspace || loading) return;
      agents = nextAgents;
      reconcileNow();
    },

    /** Unload the workspace (e.g. an empty TaskWorkspace is shown), saving it first. */
    clear(): void {
      saveNow();
      generation++;
      loading = false;
      layout = null;
      workspace = null;
      focusedSessionId = null;
    },

    // ─── Opening resources ────────────────────────────────────────────────────

    /**
     * Open a shell tab for a session in a pane (the focused one by default), or
     * in a new pane split off the focused one, and focus that pane. Returns its
     * pty key, or null when there is no pane to put it in.
     */
    openShell(
      sessionId: string,
      where: { paneId?: string; split?: SplitDirection } = {},
    ): string | null {
      // While another workspace loads, the shown layout is about to be replaced.
      if (!layout || loading) return null;
      let base = layout;
      let paneId = where.paneId ?? layout.focusedLeafId;
      if (where.split) {
        const split = tree.splitFocused(layout, where.split);
        if (!split) return null;
        base = split.layout;
        paneId = split.leafId;
      }
      if (!tree.findLeaf(base, paneId)) return null;
      const ptyKey = shellPtyKey(sessionId, allocateShellIndex(sessionId));
      commit(tree.focusLeaf(tree.addTab(base, paneId, shellTab(ptyKey, "Shell")), paneId));
      saveNow();
      return ptyKey;
    },

    /**
     * Open a file with the terminal editor in a new shell tab of the focused
     * pane. Its command is reserved before the tab exists, so the terminal that
     * mounts for it starts the editor rather than a bare shell. Null when the
     * pane went away, or another workspace started loading, while the command
     * was being resolved.
     */
    async openTerminalEditor(sessionId: string, filePath: string): Promise<string | null> {
      const paneId = loading ? null : layout?.focusedLeafId;
      if (!paneId) return null;
      const command = await deps.getTerminalCommand(sessionId, filePath);
      return openCommandTab(sessionId, paneId, command, fileName(filePath));
    },

    /** Open a shell tab in the focused pane that runs `command` when its PTY spawns, and focus it. */
    openCommand(sessionId: string, command: TabCommand, label: string): string | null {
      const paneId = loading ? null : layout?.focusedLeafId;
      return paneId ? openCommandTab(sessionId, paneId, command, label, true) : null;
    },

    /** What a shell tab runs first when its PTY spawns: a terminal editor, or a provider's handoff. */
    pendingCommand: (ptyKey: string): TabCommand | undefined => pendingCommands.get(ptyKey),
    isStarting: (ptyKey: string): boolean => pendingCommands.has(ptyKey),

    /** A shell tab's PTY spawned; its pending command has been consumed. */
    shellStarted(ptyKey: string): void {
      pendingCommands.delete(ptyKey);
    },

    /** Open the session's diff in the focused pane, or focus it. Never closes it. */
    openDiff,

    /** Close the diff when it is the front tab of its pane; otherwise open or focus it. */
    toggleDiff(sessionId: string): ResourceOpen {
      const existing = tree.findTab(layout, diffPtyKey(sessionId));
      if (layout && existing?.leaf.activeTab === diffPtyKey(sessionId)) {
        commit(tree.removeTab(layout, diffPtyKey(sessionId)));
        return "closed";
      }
      return openDiff(sessionId);
    },

    /** Open a file in the embedded editor, focusing its tab when already open. */
    openEditor(sessionId: string, filePath: string): ResourceOpen {
      if (!layout || loading) return "unavailable";
      const ptyKey = editorPtyKey(sessionId, filePath);
      if (tree.findTab(layout, ptyKey)) {
        commit(tree.focusTab(layout, ptyKey));
        return "focused";
      }
      commit(
        tree.addTab(layout, layout.focusedLeafId, {
          ptyKey,
          label: fileName(filePath),
          icon: "file",
          type: "editor",
          filePath,
        }),
      );
      return "opened";
    },

    // ─── Closing ──────────────────────────────────────────────────────────────

    /**
     * Close a tab. A shell is killed on the backend first and stays in the layout
     * if that rejects (the rejection propagates). Agent tabs are left to the
     * caller, which parks their session.
     */
    async closeTab(ptyKey: string): Promise<TabClose> {
      const found = tree.findTab(layout, ptyKey);
      if (!layout || !found) return "missing";
      switch (found.tab.type) {
        case "agent":
          return "agent";
        case "diff":
        case "editor":
          commit(tree.removeTab(layout, ptyKey));
          return "closed";
        case "shell":
          if (pendingCommands.has(ptyKey)) return "starting";
          await closeShell(ptyKey);
          return "closed";
      }
    },

    /**
     * A shell's process exited by itself.
     */
    async shellExited(ptyKey: string): Promise<void> {
      await dropShell(ptyKey, true);
    },

    /**
     * Close a shell tab whatever its state, for a session leaving the app. False when the
     * shown layout does not hold it; the caller then ends its process directly.
     */
    async discardShell(ptyKey: string): Promise<boolean> {
      if (!layout || tree.findTab(layout, ptyKey)?.tab.type !== "shell") return false;
      pendingCommands.delete(ptyKey);
      await closeShell(ptyKey);
      return true;
    },

    /** A shell tab's PTY failed to spawn: nothing runs, so its tab goes even if the backend close rejects. */
    async shellFailedToStart(ptyKey: string): Promise<void> {
      await dropShell(ptyKey, false);
    },

    // ─── Navigation and arrangement ───────────────────────────────────────────

    /** Bring a tab forward and focus its pane. */
    focusTab(ptyKey: string): TabEntry | null {
      if (!layout) return null;
      commit(tree.focusTab(layout, ptyKey));
      return tree.findTab(layout, ptyKey)?.tab ?? null;
    },

    focusPane(paneId: string): void {
      if (layout) commit(tree.focusLeaf(layout, paneId));
    },

    /** Move the focused pane's front tab by `delta`; returns the new front tab, or null if none moved. */
    cycleTab(delta: number): TabEntry | null {
      if (!layout) return null;
      const before = layout;
      commit(tree.cycleTab(layout, delta));
      return layout === before ? null : tree.focusedTabOf(layout);
    },

    /** Close a pane (the focused one by default), moving its tabs to its sibling. */
    closePane(paneId?: string): void {
      if (layout) commit(tree.closePane(layout, paneId ?? layout.focusedLeafId));
    },

    focusDirection(direction: NavDirection): void {
      if (layout) commit(tree.focusDirection(layout, direction));
    },

    moveFocusedTab(direction: NavDirection): void {
      if (layout) commit(tree.moveFocusedTab(layout, direction));
    },

    moveTab(ptyKey: string, paneId: string, index?: number): void {
      if (layout) commit(tree.moveTab(layout, ptyKey, paneId, index));
    },

    /** Split a pane on one side and move a tab into the new pane. */
    splitWithTab(ptyKey: string, paneId: string, side: NavDirection): void {
      if (layout) commit(tree.splitWithTab(layout, ptyKey, paneId, side));
    },

    setRatio(splitId: string, ratio: number): void {
      if (layout) commit(tree.setRatio(layout, splitId, ratio));
    },

    /** Title a tab (e.g. from a shell's OSC title); agent relabeling keeps it. */
    setTabTitle(ptyKey: string, label: string): void {
      if (layout) commit(tree.setTabTitle(layout, ptyKey, label));
    },
  };
}

function fileName(filePath: string): string {
  return filePath.split("/").pop() || filePath;
}

/** The tabs of a pane as a tab strip renders them. */
export function toPaneTabs(
  leaf: LeafNode,
  detailFor: (tab: TabEntry) => string | undefined = () => undefined,
): PaneTab[] {
  return leaf.tabs.map((tab) => ({
    id: tab.ptyKey,
    label: tab.label,
    detail: detailFor(tab),
    icon: tab.icon,
  }));
}

/**
 * What a restored layout implies for the session selection. Its saved front tab
 * is the memory of which agent you were last on, and need not be the session the
 * load started for. The two must not be left disagreeing: nothing later
 * reconciles them, and while they disagree no terminal pane qualifies as focused
 * (see `isTerminalPaneFocused` in ./terminal-focus).
 */
export type RestoredLayoutSelection =
  | { kind: "keep_selection" }
  | { kind: "focus_selected_session" }
  | { kind: "adopt_restored_session"; sessionId: string };

export function resolveRestoredLayoutSelection(input: {
  /**
   * Session of the restored layout's front tab, but only when it still exists in
   * this workspace. A remembered session that is gone must never be adopted: it
   * would select a session nothing can resolve, stalling every later repair.
   */
  liveRestoredSessionId: string | null;
  selectedSessionId: string;
  restoredTreeHasTabForSelectedSession: boolean;
  selectionIsExplicit: boolean;
}): RestoredLayoutSelection {
  const selectionWins: RestoredLayoutSelection = input.restoredTreeHasTabForSelectedSession
    ? { kind: "focus_selected_session" }
    : // No tab yet: reconciliation adds one and the preferred tab makes it active.
      // Adopting here would silently discard the selection.
      { kind: "keep_selection" };

  if (input.liveRestoredSessionId === input.selectedSessionId) return { kind: "keep_selection" };
  if (input.liveRestoredSessionId === null) return selectionWins;
  // A real user choice is never overridden by the remembered tab.
  if (input.selectionIsExplicit) return selectionWins;
  return { kind: "adopt_restored_session", sessionId: input.liveRestoredSessionId };
}

const closeShellPty = (sessionId: string, index: number) => pty.closeTab(sessionId, index);

/** The app's TaskWorkspace layout. */
export const taskWorkspaceLayout = createTaskWorkspaceLayout({
  store: {
    load: (workspace) =>
      workspace.kind === "task"
        ? sessionsApi.getTaskWorkspaceLayout(workspace.projectId, workspace.taskKey)
        : sessionsApi.getLayout(workspace.sessionId),
    save: (workspace, layoutJson) =>
      workspace.kind === "task"
        ? sessionsApi.saveTaskWorkspaceLayout(workspace.projectId, workspace.taskKey, layoutJson)
        : sessionsApi.saveLayout(workspace.sessionId, layoutJson),
  },
  closeShell: closeShellPty,
  shellClosed: (ptyKey, exited) => providerHandoff.shellClosed(ptyKey, exited),
  getTerminalCommand: (sessionId, filePath) => editorApi.getTerminalCommand(sessionId, filePath),
  disposeView: disposeTerminalView,
  onSaveError: (workspace, error) =>
    console.warn("Failed to save workspace layout", workspace.key, error),
});

/** Provider sessions continuing in this layout's terminal tabs; built here, as each calls the other. */
export const providerHandoff = createProviderHandoff({
  api: {
    handoff: (sessionId) => providerSessions.handoff(sessionId),
    handback: (sessionId) => providerSessions.handback(sessionId),
  },
  discardTab: (ptyKey) => taskWorkspaceLayout.discardShell(ptyKey),
  closeTerminal: async (ptyKey) => {
    const parts = parsePtyKey(ptyKey);
    if (parts?.kind === "shell") await closeShellPty(parts.sessionId, parts.index);
  },
  notify: (message) => showSnackbar(message),
});
