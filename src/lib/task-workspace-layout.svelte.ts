/**
 * TaskWorkspace layout: the tabs and splits of the loaded TaskWorkspace.
 *
 * Owns the layout tree (see ./layout-tree), which workspace it belongs to, and its
 * persistence. Terminal tabs (shells, terminal editors, handoffs) are the backend's
 * (ADR-0015): it hands out their indices and reports their end once, and the layout
 * only places them and drops them.
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
import { sessionTaskProjectId, type Session, type TabEndReason, type TabSpec } from "./types";
import { disposeTerminalView } from "./terminal-views";
import { createProviderHandoff } from "./provider-handoff";
import { showSnackbar } from "./snackbar.svelte";

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

/** The backend's terminal tabs. */
export interface TerminalTabs {
  /** Reserve a tab running `spec`; resolves an index the session never had. */
  open: (sessionId: string, spec: TabSpec) => Promise<number>;
  /** Rejects, leaving the tab live, when its process could not be ended. */
  close: (sessionId: string, index: number) => Promise<unknown>;
  /** The tabs among `ptyKeys` that ended. */
  endedAmong: (ptyKeys: string[]) => Promise<string[]>;
}

export interface TaskWorkspaceLayoutDeps {
  store: LayoutStore;
  tabs: TerminalTabs;
  /** A terminal tab ended, as the backend reports once per tab; it never rejects. */
  tabEnded?: (ptyKey: string, reason: TabEndReason, error?: string) => Promise<void>;
  /** A loaded layout holds a provider handoff's terminal, so its close must still hand back. */
  handoffRestored?: (ptyKey: string) => void;
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

/** Outcome of closing a tab. `agent` tabs are durable sessions the caller parks. */
export type TabClose = "closed" | "agent" | "missing";

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
  /** Terminal tabs that ended while a layout loaded, which it may still hold. */
  const endedWhileLoading = new Set<string>();

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

  /**
   * Reserve a terminal tab on the backend and put it in the layout with `place`. Null when
   * the layout changed so that it has no place anymore; its reservation then ends.
   */
  async function openTerminalTab(
    sessionId: string,
    spec: TabSpec,
    place: (base: Layout, ptyKey: string) => Layout | null,
  ): Promise<string | null> {
    if (!layout || loading) return null;
    const shown = workspace;
    const index = await deps.tabs.open(sessionId, spec);
    const ptyKey = shellPtyKey(sessionId, index);
    const next = layout && !loading && workspace === shown ? place(layout, ptyKey) : null;
    if (!next) {
      deps.tabs
        .close(sessionId, index)
        .catch((error) => console.warn("Failed to end an unplaced terminal tab", ptyKey, error));
      return null;
    }
    commit(next);
    saveNow();
    return ptyKey;
  }

  /** Reserve a terminal tab running `spec` in a pane, so the terminal that mounts for it starts it. */
  function openCommandTab(
    sessionId: string,
    paneId: string,
    spec: TabSpec,
    label: string,
    focus = false,
    handoff = false,
  ): Promise<string | null> {
    if (!layout || !tree.findLeaf(layout, paneId)) return Promise.resolve(null);
    return openTerminalTab(sessionId, spec, (base, ptyKey) => {
      if (!tree.findLeaf(base, paneId)) return null;
      const tab = { ...shellTab(ptyKey, label), ...(handoff ? { handoff: true } : {}) };
      const next = tree.addTab(base, paneId, tab);
      return focus ? tree.focusLeaf(next, paneId) : next;
    });
  }

  function shellTab(ptyKey: string, label: string): TabEntry {
    return { ptyKey, label, icon: "terminal", type: "shell" };
  }

  /** A terminal tab ended: it leaves the layout, and the backend is never asked about it again. */
  function dropTerminal(ptyKey: string): void {
    if (loading) endedWhileLoading.add(ptyKey);
    if (layout && tree.findTab(layout, ptyKey)) {
      commit(tree.removeTab(layout, ptyKey));
      saveNow();
    }
    deps.disposeView(ptyKey);
  }

  /**
   * Close a terminal tab, dropping it as soon as the backend ends it. What its end means to
   * others waits for its `tab-ended`, which tells why it ended.
   */
  async function closeTerminal(ptyKey: string): Promise<void> {
    const parts = parsePtyKey(ptyKey);
    if (parts?.kind !== "shell") return;
    await deps.tabs.close(parts.sessionId, parts.index);
    dropTerminal(ptyKey);
  }

  /** The terminal tabs of a saved layout that ended while it was not shown. */
  async function endedTabsOf(restored: Layout): Promise<string[]> {
    const keys = tree
      .tabsOf(restored)
      .filter((tab) => tab.type === "shell")
      .map((tab) => tab.ptyKey);
    return keys.length > 0 ? deps.tabs.endedAmong(keys) : [];
  }

  function pruneEnded(restored: Layout, ended: readonly string[]): Layout | null {
    let result: Layout | null = restored;
    for (const ptyKey of [...ended, ...endedWhileLoading]) {
      if (result) result = tree.removeTab(result, ptyKey);
    }
    return result;
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
      endedWhileLoading.clear();
      let restored: Layout | null = null;
      let ended: string[] = [];
      try {
        const json = await deps.store.load(target);
        restored = json ? tree.restoreLayout(json) : null;
        if (restored) ended = await endedTabsOf(restored);
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
      const pruned = restored && pruneEnded(restored, ended);
      layout = pruned || tree.createLayout(agentTabs(), selectedTab);
      workspace = target;
      focusedSessionId = adoptedSessionId ?? options.selectedSessionId;
      loading = false;
      reconcileNow();
      // After reconcile, so tabs of sessions gone from the workspace are never adopted.
      for (const tab of tree.tabsOf(layout)) if (tab.handoff) deps.handoffRestored?.(tab.ptyKey);
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
    ): Promise<string | null> {
      // While another workspace loads, the shown layout is about to be replaced.
      return openTerminalTab(sessionId, { kind: "shell" }, (current, ptyKey) => {
        let base = current;
        let paneId = where.paneId ?? current.focusedLeafId;
        if (where.split) {
          const split = tree.splitFocused(current, where.split);
          if (!split) return null;
          base = split.layout;
          paneId = split.leafId;
        }
        if (!tree.findLeaf(base, paneId)) return null;
        return tree.focusLeaf(tree.addTab(base, paneId, shellTab(ptyKey, "Shell")), paneId);
      });
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
      return openCommandTab(sessionId, paneId, { kind: "command", command }, fileName(filePath));
    },

    /** Open a terminal tab in the focused pane that runs `spec` when it starts, and focus it. */
    openCommand(
      sessionId: string,
      spec: TabSpec,
      label: string,
      options: { handoff?: boolean } = {},
    ): Promise<string | null> {
      const paneId = layout?.focusedLeafId;
      return paneId
        ? openCommandTab(sessionId, paneId, spec, label, true, options.handoff)
        : Promise.resolve(null);
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
     * Close a tab. A terminal tab is ended on the backend first and stays in the layout
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
          await closeTerminal(ptyKey);
          return "closed";
      }
    },

    /** The backend reported a terminal tab's end (`tab-ended`). */
    async tabEnded(ptyKey: string, reason: TabEndReason, error?: string): Promise<void> {
      dropTerminal(ptyKey);
      await deps.tabEnded?.(ptyKey, reason, error);
    },

    /** A terminal tab found ended when its terminal attached; its end was reported already. */
    dropEnded: dropTerminal,

    /**
     * Close a terminal tab whatever its state, for a session leaving the app. False when the
     * shown layout does not hold it; the caller then ends its process directly.
     */
    async discardTerminal(ptyKey: string): Promise<boolean> {
      if (!layout || tree.findTab(layout, ptyKey)?.tab.type !== "shell") return false;
      await closeTerminal(ptyKey);
      return true;
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

const closeTerminalPty = (sessionId: string, index: number) => pty.closeTab(sessionId, index);

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
  tabs: {
    open: (sessionId, spec) => pty.openTab(sessionId, spec),
    close: closeTerminalPty,
    endedAmong: (ptyKeys) => pty.endedTabs(ptyKeys),
  },
  tabEnded: async (ptyKey, reason, error) => {
    const explained = await providerHandoff.tabEnded(ptyKey, reason, error);
    if (reason === "failed_to_start" && !explained) {
      showSnackbar(`Failed to start terminal${error ? `: ${error}` : ""}`, "error");
    }
  },
  handoffRestored: (ptyKey) => {
    void providerHandoff
      .adopt(ptyKey)
      .catch((error) => console.warn("Failed to restore a handoff terminal", ptyKey, error));
  },
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
  discardTab: (ptyKey) => taskWorkspaceLayout.discardTerminal(ptyKey),
  closeTerminal: async (ptyKey) => {
    const parts = parsePtyKey(ptyKey);
    if (parts?.kind === "shell") await closeTerminalPty(parts.sessionId, parts.index);
  },
  isProgramRunning: (ptyKey) => pty.isProgramRunning(ptyKey),
  notify: (message) => showSnackbar(message),
});
