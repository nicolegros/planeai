<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { listen } from "@tauri-apps/api/event";
  import { sessions as sessionsApi, pty, notify, sessionLogs, editor as editorApi, updater } from "./lib/api";
  import type { Session, Project, TaskItem } from "./lib/types";
  import { focusEditor, focusTerminal, refocusTerminal, focusExplorer, focusSidebar, getActiveZone, toggleExplorerFocus } from "./lib/focus.svelte";
  import * as projectStore from "./lib/project-store.svelte";
  import * as taskStore from "./lib/task-store.svelte";
  import { installKeyboardRouter, matchChord, MOD_LABEL, isPlatformMod, MOD_ENTER_HINT } from "./lib/keyboard";
  import { findPluginShortcut } from "./lib/plugin-shortcuts";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import * as navCycle from "./lib/session-nav-cycle.svelte";
  import { computeSidebarSessionOrder, isLoopId, parseLoopId, isTaskWorkspaceId, parseTaskWorkspaceId, toTaskWorkspaceId } from "./lib/sidebar-session-order";
  import { isTerminal, isActive as isLoopActive } from "./lib/loop-status";
  import { loadSettings, getSettings, isDark } from "./lib/settings.svelte";
  import { openFileWithConfiguredEditor } from "./lib/file-editor";
  import { loadTheme } from "./lib/theme-loader";
  import { getSnackbarMessage, getSnackbarType, dismissSnackbar, showSnackbar } from "./lib/snackbar.svelte";
  import { Dialog } from "bits-ui";
  import Titlebar from "./components/Titlebar.svelte";
  import KeyboardHelperBar from "./components/KeyboardHelperBar.svelte";
  import UnifiedSidebar from "./components/UnifiedSidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import TaskForm from "./components/TaskForm.svelte";
  import Terminal from "./components/Terminal.svelte";
  import TabSwitcher from "./components/TabSwitcher.svelte";
  import CommandMenu from "./components/CommandMenu.svelte";
  import ReviewTab from "./components/ReviewTab.svelte";
  import EditorTab from "./components/EditorTab.svelte";
  import FileExplorer from "./components/FileExplorer.svelte";
  import KeyboardShortcuts from "./components/KeyboardShortcuts.svelte";
  import SharedDialog from "./components/ui/Dialog.svelte";
  import FormDialog from "./components/ui/FormDialog.svelte";
  import LogViewer from "./components/LogViewer.svelte";
  import PostMergePrompt from "./components/PostMergePrompt.svelte";
  import LoopForm from "./components/LoopForm.svelte";
  import LoopDashboard from "./components/LoopDashboard.svelte";
  import EmptyTaskWorkspace from "./components/EmptyTaskWorkspace.svelte";
  import PluginContributionHost from "./components/PluginContributionHost.svelte";
  import type { PluginInventory, PluginSessionAction, PluginSessionAdvisory, PluginSessionCompletion, PluginUiContribution } from "./lib/types";
  import * as loopStore from "./lib/loop-store.svelte";
  import { loops as loopsApi, plugins as pluginsApi } from "./lib/api";
  import { focusMergePrompt, getPrompt, showMergePrompt } from "./lib/post-merge-prompt.svelte";
  import { getTabs, getActiveTabIndex, addTab, removeTab } from "./lib/session-tabs.svelte";
  import { consumePendingTerminalEditorCommand, getPendingTerminalEditorCommand, queueTerminalEditor, rollbackPendingTerminalEditor } from "./lib/terminal-editor";
  import { ptyKeyToSessionId, reconcileWorkspaceTabs } from "./lib/workspace-tabs";
  import { addShellTabToLeaf, openEditorResource, openShellResourceInFocusedLeaf, openShellResourceInNewPane, toggleDiffResource } from "./lib/workspace-resources";
  import { saveActiveEditorResource } from "./lib/editor-resources";
  import { getMruList, isMounted as poolIsMounted } from "./lib/mru.svelte";
  import * as orchestrator from "./lib/session-orchestrator.svelte";
  import UpdateToast from "./components/UpdateToast.svelte";
  import { initUpdateListener, focusUpdateToast, getUpdateState, setLaunchUpdateAvailable } from "./lib/updater.svelte";
  import SplitContainer from "./components/SplitContainer.svelte";
  import TabStrip from "./components/TabStrip.svelte";
  import * as splitTree from "./lib/split-tree.svelte";
  import type { LeafNode } from "./lib/split-tree.svelte";

  // ─── UI-only state ──────────────────────────────────────────────────────────
  let showProjectForm = $state(false);
  let showSessionForm = $state(false);
  let showTaskForm = $state(false);
  let sidebarVisible = $state(true);
  let commandMenuOpen = $state(false);
  let commandMenuFileMode = $state(false);
  let showNewItemModal = $state(false);
  let showShortcuts = $state(false);
  let showHookPrompt = $state(false);
  let showQuitConfirm = $state(false);
  let showLoopForm = $state(false);
  let quitDirectCount = $state(0);
  let fileExplorerVisible = $state(false);
  let showLogViewer = $state(false);
  let activePluginId = $state<string | null>(null);
  let activeContributionId = $state<string | null>(null);
  let pluginInventory = $state<import("./lib/types").PluginInventory[]>([]);
  const pendingShellCommands = new Map<string, string>();
  let pluginSessionActions = $state<PluginSessionAction[]>([]);
  let pluginSessionActionsRevision = 0;
  let terminalFocusRequest = $state<{ id: number; sessionId: string } | null>(null);

  let modalPluginId = $state<string | null>(null);
  let modalContributionId = $state<string | null>(null);

  let logViewerEnabled = $state(false);
  let sessionToDelete = $state<Session | null>(null);
  let projectToDelete = $state<Project | null>(null);
  let projectToEdit = $state<Project | null>(null);
  let loopToDelete = $state<import("./lib/types").LoopRunSummary | null>(null);
  let renamingSessionId = $state<string | null>(null);
  let taskPrefill = $state<{ key: string; title: string; description: string; branch: string; name: string; prompt: string; baseBranch?: string; projectId?: string | null } | null>(null);
  let selectedTaskWorkspace = $state<{ task: TaskItem; project: Project } | null>(null);
  let taskWorkspaceToEdit = $state<{ task: TaskItem; project: Project } | null>(null);
  let archivedTaskSessions = $state<Session[]>([]);
  // Task and loop identities are UI-only; persisted MRU accepts real session IDs only.
  let workspaceMru = $state<string[]>([]);

  // ─── Derived from orchestrator ──────────────────────────────────────────────
  const projects = $derived(projectStore.getProjects());
  const sessions = $derived(orchestrator.getSessions());
  const activeSessionId = $derived(orchestrator.getActiveSessionId());
  const agentStates = $derived(orchestrator.getAgentStates());
  const diffTabOpen = $derived(orchestrator.getDiffTabOpen());
  const editorTabOpen = $derived(orchestrator.getEditorTabOpen());
  const diffFileName = $derived(orchestrator.getDiffFileName());
  const editorFileName = $derived(orchestrator.getEditorFileName());
  const editorModified = $derived(orchestrator.getEditorModified());
  const symphonyStatus = $derived(orchestrator.getSymphonyStatus());
  const zone = $derived(getActiveZone());
  const activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);
  const activeTaskWorkspace = $derived.by(() => {
    if (selectedTaskWorkspace) return selectedTaskWorkspace;
    if (!activeSession?.task_key) return null;
    const project = projects.find((candidate) => candidate.id === activeSession.project_id);
    const task = project ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === activeSession.task_key) : undefined;
    return task && project ? { task, project } : null;
  });
  const activeTaskSessions = $derived(
    activeTaskWorkspace
      ? sessions.filter((session) => session.project_id === activeTaskWorkspace.project.id && session.task_key === activeTaskWorkspace.task.key)
      : [],
  );
  const isEmptyTaskWorkspace = $derived(!!activeTaskWorkspace && activeTaskSessions.length === 0);
  const activePluginSessionContext = $derived(activeSession ? {
    id: activeSession.id,
    projectId: activeSession.project_id,
    branch: activeSession.branch,
    baseBranch: activeSession.base_branch,
    status: activeSession.status,
    provider: activeSession.provider,
    taskKey: activeSession.task_key,
  } : undefined);
  const activeLoopId = $derived(loopStore.getActiveLoopId());
  const activePlugin = $derived(pluginInventory.find((plugin) => plugin.id === activePluginId) ?? null);
  const activeContribution = $derived(activePlugin?.ui_contributions.find((contribution) => contribution.id === activeContributionId) ?? null);
  const modalPlugin = $derived(pluginInventory.find((plugin) => plugin.id === modalPluginId) ?? null);
  const modalContribution = $derived(modalPlugin?.ui_contributions.find((contribution) => contribution.id === modalContributionId) ?? null);
  const comparePluginContribution = (left: { plugin: PluginInventory; contribution: PluginUiContribution }, right: { plugin: PluginInventory; contribution: PluginUiContribution }) =>
    (left.contribution.order ?? 0) - (right.contribution.order ?? 0) || left.plugin.name.localeCompare(right.plugin.name) || left.plugin.id.localeCompare(right.plugin.id) || left.contribution.id.localeCompare(right.contribution.id);
  const sidebarPluginContributions = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => ["sidebar.header", "sidebar.navigation", "sidebar.section", "sidebar.footer"].includes(contribution.placement)).map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const sessionIndicatorContributions = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => contribution.placement === "session.indicator").map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const mainPaneCommands = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => contribution.placement === "main-pane").map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const sessionPanelCommands = $derived(
    activeSession
      ? pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
          plugin.ui_contributions.filter((contribution) => contribution.placement === "session.panel").map((contribution) => ({ plugin, contribution })),
        ).sort(comparePluginContribution)
      : [],
  );
  const titlebarContributions = $derived(
    activeSession
      ? pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
          plugin.ui_contributions.filter((contribution) => contribution.placement === "titlebar").map((contribution) => ({ plugin, contribution })),
        ).sort(comparePluginContribution)
      : [],
  );
  const pluginCommands = $derived([...mainPaneCommands, ...sessionPanelCommands]);
  const interactionPluginContributions = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => contribution.placement === "interaction").map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const activeProjectName = $derived(activeSession ? (projects.find((p) => p.id === activeSession.project_id)?.name ?? null) : null);
  const activeSessionName = $derived(activeSession ? (activeSession.name || activeSession.branch) : null);

  // Session IDs in sidebar display order (includes loop:<id> entries)
  const sidebarSessionOrder = $derived(computeSidebarSessionOrder(
    projects,
    sessions,
    taskStore.getTasksByProject(),
    !!getSettings().hide_done_tasks,
    Object.fromEntries(projects.map((p) => [p.id, loopStore.getLoopsForProject(p.id)])),
    Object.fromEntries(projects.flatMap((p) => loopStore.getLoopsForProject(p.id)).map((l) => [l.id, loopStore.getSessionsForLoop(l.id)])),
    new Set(projects.flatMap((p) => loopStore.getLoopsForProject(p.id)).flatMap((l) => loopStore.getSessionsForLoop(l.id).map((s) => s.session_id))),
  ));

  // Pre-compute titlebar tabs to avoid IIFE re-evaluation on every render
  const titlebarTabs = $derived.by(() => {
    if (!activeSessionId) return [];
    // When split tree is active with a single leaf, derive tabs from the leaf
    const tree = splitTree.getTree();
    if (tree && tree.type === "leaf" && tree.tabs.length > 0) {
      return getLeafTabInfo(tree);
    }
    // Fallback to session-tabs store (before tree is initialized)
    const shellTabs = getTabs(activeSessionId).map(t => t.index === 0 ? { ...t, label: activeSession?.provider || getSettings().default_provider || "Agent" } : t);
    const extra: import("./lib/session-tabs.svelte").Tab[] = [];
    if (diffTabOpen[activeSessionId]) extra.push({ id: `${activeSessionId}:diff`, index: -1, label: diffFileName[activeSessionId] || "Diff", icon: "git-compare" });
    if (editorTabOpen[activeSessionId]) extra.push({ id: `${activeSessionId}:editor`, index: -2, label: editorFileName[activeSessionId] || "Editor", icon: "file", modified: editorModified[activeSessionId] || false });
    return [...shellTabs, ...extra];
  });

  // ─── Split tree ─────────────────────────────────────────────────────────────
  const splitTreeNode = $derived(splitTree.getTree());
  const hasMultiplePanes = $derived(splitTreeNode !== null && splitTreeNode.type === "split");
  const titlebarActiveTabIdx = $derived.by(() => {
    const tree = splitTree.getTree();
    if (tree?.type === "leaf") {
      return getLeafTabInfo(tree).find((tab) => tab.id === tree.activeTab)?.index ?? 0;
    }
    return orchestrator.getUnifiedActiveIndex();
  });
  const titlebarActiveTabId = $derived.by(() => {
    const tree = splitTree.getTree();
    return tree?.type === "leaf" ? tree.activeTab : undefined;
  });

  // Initialize tree when sessions first load (single leaf with all session IDs)
  let splitTreeInitialized = $state(false);

  $effect(() => {
    if (!splitTreeInitialized && sessions.length > 0) {
      splitTreeInitialized = true;
    }
  });

  // ─── TaskWorkspace split layout (DB-backed) ─────────────────────────────────
  type WorkspaceIdentity = { key: string; projectId: string; taskKey: string | null };
  let lastTreeWorkspace = $state<WorkspaceIdentity | null>(null);
  let lastFocusedWorkspaceSessionId: string | null = null;
  let loadGeneration = 0; // not reactive - just a counter for staleness
  let loadingLayout = $state(false); // suppress auto-save and stale-tab cleanup during load

  function workspaceForSession(sessionId: string): WorkspaceIdentity | null {
    const session = sessions.find((candidate) => candidate.id === sessionId);
    if (!session) return null;
    return session.task_key
      ? { key: `task:${session.project_id}:${session.task_key}`, projectId: session.project_id, taskKey: session.task_key }
      : { key: `session:${session.id}`, projectId: session.project_id, taskKey: null };
  }

  function workspaceSessions(workspace: WorkspaceIdentity): Session[] {
    if (!workspace.taskKey) {
      const sessionId = workspace.key.slice("session:".length);
      return sessions.filter((session) => session.id === sessionId);
    }
    return sessions.filter((session) => session.project_id === workspace.projectId && session.task_key === workspace.taskKey);
  }

  // Selecting another agent in the same task keeps the shared workspace loaded and
  // simply makes that agent terminal the focused context.
  $effect(() => {
    if (!activeSessionId || !splitTreeInitialized) return;
    const workspace = workspaceForSession(activeSessionId);
    if (!workspace) return;
    const focusedSessionChanged = activeSessionId !== lastFocusedWorkspaceSessionId;
    if (workspace.key === lastTreeWorkspace?.key) {
      if (!focusedSessionChanged) return;
      lastFocusedWorkspaceSessionId = activeSessionId;
      const existing = splitTree.findTab(activeSessionId);
      if (existing) {
        if (existing.leaf.activeTab !== activeSessionId) splitTree.focusTab(activeSessionId);
        return;
      }
      const tree = splitTree.getTree();
      const entries = buildTabEntriesForWorkspace(workspace);
      if (tree?.type === "leaf") splitTree.replaceRootLeafTabs(entries, activeSessionId);
      return;
    }

    if (lastTreeWorkspace) saveSplitTreeToDb();
    loadLayoutForWorkspace(workspace, activeSessionId);
  });

  async function loadLayoutForWorkspace(workspace: WorkspaceIdentity, focusedSessionId: string): Promise<void> {
    loadingLayout = true;
    const gen = ++loadGeneration;
    try {
      const layoutJson = workspace.taskKey
        ? await sessionsApi.getTaskWorkspaceLayout(workspace.projectId, workspace.taskKey)
        : await sessionsApi.getLayout(focusedSessionId);
      if (gen !== loadGeneration) { loadingLayout = false; return; }
      if (layoutJson) {
        const data = JSON.parse(layoutJson);
        if (isValidSerializedTree(data)) {
          splitTree.deserialize(data);
          lastTreeWorkspace = workspace;
          lastFocusedWorkspaceSessionId = focusedSessionId;
          loadingLayout = false;
          requestFocusedTerminalFocus();
          return;
        }
      }
    } catch (e) {
      console.warn("Failed to load workspace layout", workspace.key, e);
    }
    if (gen !== loadGeneration) { loadingLayout = false; return; }
    const entries = buildTabEntriesForWorkspace(workspace);
    if (!splitTree.replaceRootLeafTabs(entries, focusedSessionId)) splitTree.initTree(entries, focusedSessionId);
    lastTreeWorkspace = workspace;
    lastFocusedWorkspaceSessionId = focusedSessionId;
    loadingLayout = false;
  }

  /** Validate deserialized tree structure to prevent corrupt data from crashing. */
  function isValidSerializedTree(data: unknown): data is import("./lib/split-tree.svelte").SerializedTree {
    if (!data || typeof data !== "object") return false;
    const d = data as Record<string, unknown>;
    if (typeof d.focusedLeafId !== "string") return false;
    if (!d.tree || typeof d.tree !== "object") return false;
    if (!isValidTreeNode(d.tree)) return false;
    // Migrate: ensure all tabs have a type field (for trees saved before type was added)
    migrateTreeTypes(d.tree as import("./lib/split-tree.svelte").TreeNode);
    // Validate focusedLeafId references an existing leaf
    if (!leafExistsInNode(d.tree as import("./lib/split-tree.svelte").TreeNode, d.focusedLeafId as string)) {
      // Fallback to first leaf in the tree
      const firstLeaf = findFirstLeafId(d.tree as import("./lib/split-tree.svelte").TreeNode);
      if (!firstLeaf) return false;
      d.focusedLeafId = firstLeaf;
    }
    return true;
  }

  function leafExistsInNode(node: import("./lib/split-tree.svelte").TreeNode, id: string): boolean {
    if (node.type === "leaf") return node.id === id;
    return leafExistsInNode(node.children[0], id) || leafExistsInNode(node.children[1], id);
  }

  function findFirstLeafId(node: import("./lib/split-tree.svelte").TreeNode): string | null {
    if (node.type === "leaf") return node.id;
    return findFirstLeafId(node.children[0]);
  }

  function isValidTreeNode(node: unknown, depth = 0): boolean {
    if (depth > 50) return false;
    if (!node || typeof node !== "object") return false;
    const n = node as Record<string, unknown>;
    if (typeof n.id !== "string") return false;
    if (n.type === "leaf") {
      return Array.isArray(n.tabs) && typeof n.activeTab === "string";
    }
    if (n.type === "split") {
      return typeof n.direction === "string"
        && typeof n.ratio === "number" && n.ratio >= 0 && n.ratio <= 1
        && Array.isArray(n.children) && n.children.length === 2
        && isValidTreeNode(n.children[0], depth + 1) && isValidTreeNode(n.children[1], depth + 1);
    }
    return false;
  }

  /** Backfill type field on TabEntry for trees saved before type was introduced. */
  function migrateTreeTypes(node: import("./lib/split-tree.svelte").TreeNode): void {
    if (node.type === "leaf") {
      for (const tab of node.tabs) {
        if (!tab.type) {
          if (tab.ptyKey.includes(":diff")) tab.type = "diff";
          else if (tab.ptyKey.includes(":editor:")) {
            tab.type = "editor";
            // Restore filePath from ptyKey format: sessionId:editor:filePath
            if (!tab.filePath) {
              const editorIdx = tab.ptyKey.indexOf(":editor:");
              if (editorIdx !== -1) tab.filePath = tab.ptyKey.slice(editorIdx + 8);
            }
          }
          else if (tab.ptyKey.includes(":")) tab.type = "shell";
          else tab.type = "agent";
        }
      }
    } else {
      migrateTreeTypes(node.children[0]);
      migrateTreeTypes(node.children[1]);
    }
  }

  /** Build a flat resource list for the current TaskWorkspace. */
  function buildTabEntriesForWorkspace(workspace: WorkspaceIdentity): import("./lib/split-tree.svelte").TabEntry[] {
    return workspaceSessions(workspace).flatMap((session) => {
      const sessionTabs = getTabs(session.id);
      if (sessionTabs.length === 0) {
        return [{
          ptyKey: session.id,
          label: session.name || session.branch || "Agent",
          icon: session.provider ? "bot" : "terminal",
          type: "agent" as const,
          customTitle: false,
        }];
      }
      return sessionTabs.map((tab) => ({
        ptyKey: tab.index === 0 ? session.id : `${session.id}:${tab.index}`,
        label: tab.index === 0 ? (session.name || session.branch || "Agent") : `${session.name || session.branch || "Agent"} · ${tab.customTitle ? tab.label : "Shell"}`,
        icon: tab.index === 0 ? (session.provider ? "bot" : "terminal") : "terminal",
        type: (tab.index === 0 ? "agent" : "shell") as "agent" | "shell",
        customTitle: tab.customTitle,
      }));
    });
  }

  // Reconcile restored/new task sessions and prune archived/deleted ones without
  // replacing the current split layout or its active resource tab.
  $effect(() => {
    if (loadingLayout || !lastTreeWorkspace) return;
    reconcileWorkspaceTabs({
      workspaceEntries: buildTabEntriesForWorkspace(lastTreeWorkspace),
      validSessionIds: new Set(workspaceSessions(lastTreeWorkspace).map((session) => session.id)),
      reservedPtyKeys: pendingShellCommands,
      tree: splitTree,
    });
  });

  // Drag-and-drop state
  let dragSessionId = $state<string | null>(null);

  function handleTabDragStart(e: DragEvent, ptyKey: string, leafId: string) {
    if (!ptyKey) { e.preventDefault(); return; }
    dragSessionId = ptyKey;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", ptyKey);
    }
  }

  function handleTabDrop(e: DragEvent, targetLeafId: string, insertIndex: number) {
    if (!dragSessionId) return;
    splitTree.moveSessionToLeaf(dragSessionId, targetLeafId, insertIndex);
    dragSessionId = null;
  }

  function handleTabDragOver(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  }

  // Get tab info for a leaf — preserve persistent shell indices for TabStrip callbacks.
  function getLeafTabInfo(leaf: LeafNode): import("./lib/session-tabs.svelte").Tab[] {
    return leaf.tabs.map((tabEntry, visualIndex) => ({
      id: tabEntry.ptyKey,
      index: tabIndexForEntry(tabEntry, visualIndex),
      label: tabEntry.label,
      icon: tabEntry.icon,
      customTitle: tabEntry.customTitle,
    }));
  }

  function tabIndexForEntry(tabEntry: import("./lib/split-tree.svelte").TabEntry, fallback: number): number {
    if (tabEntry.type === "agent") return 0;
    if (tabEntry.type === "diff") return -1;
    if (tabEntry.type === "editor") return -2;
    const separator = tabEntry.ptyKey.lastIndexOf(":");
    const index = Number.parseInt(tabEntry.ptyKey.slice(separator + 1), 10);
    return separator === -1 || Number.isNaN(index) ? fallback : index;
  }

  // Handle split keyboard actions
  function handleSplitAction(actionType: string): void {
    switch (actionType) {
      case "split_vertical": {
        doSplit("vertical");
        break;
      }
      case "split_horizontal": {
        doSplit("horizontal");
        break;
      }
      case "close_split": {
        const focusedLeafId = splitTree.getFocusedLeafId();
        if (focusedLeafId) splitTree.closeSplit(focusedLeafId);
        syncFocusedLeafToOrchestrator();
        break;
      }
      case "focus_split_left": splitTree.focusDirection("left"); syncFocusedLeafToOrchestrator(); break;
      case "focus_split_right": splitTree.focusDirection("right"); syncFocusedLeafToOrchestrator(); break;
      case "focus_split_up": splitTree.focusDirection("up"); syncFocusedLeafToOrchestrator(); break;
      case "focus_split_down": splitTree.focusDirection("down"); syncFocusedLeafToOrchestrator(); break;
      case "move_tab_left": splitTree.moveTabToDirection("left"); syncFocusedLeafToOrchestrator(); break;
      case "move_tab_right": splitTree.moveTabToDirection("right"); syncFocusedLeafToOrchestrator(); break;
      case "move_tab_up": splitTree.moveTabToDirection("up"); syncFocusedLeafToOrchestrator(); break;
      case "move_tab_down": splitTree.moveTabToDirection("down"); syncFocusedLeafToOrchestrator(); break;
    }
  }

  /**
   * Split the focused pane and open a shell tab in the new pane.
   * Uses the existing session's shell tab mechanism ($SHELL -l).
   */
  function doSplit(direction: "vertical" | "horizontal"): void {
    if (!activeSessionId) return;
    if (!openShellResourceInNewPane(activeSessionId, direction)) return;
    // Wait for Terminal to mount + open before refocusing
    tick().then(() => requestAnimationFrame(() => refocusTerminal()));
  }

  /** Open a new shell tab in the focused split leaf. */
  function splitNewTab(): void {
    if (!activeSessionId) return;
    if (!openShellResourceInFocusedLeaf(activeSessionId)) return;
    refocusTerminal();
  }

  /** Close the active tab in the focused split leaf. */
  async function splitCloseTab(): Promise<void> {
    const leaf = splitTree.getFocusedLeaf();
    if (!leaf || leaf.tabs.length === 0) return;

    const activeEntry = splitTree.getActiveTabEntry(leaf);
    if (!activeEntry) return;

    // Agent tabs represent durable sessions. Cmd+W archives the active agent
    // rather than destroying its worktree, so task workspaces may be empty.
    if (activeEntry.type === "agent") {
      const sessionId = ptyKeyToSessionId(activeEntry.ptyKey);
      const session = sessions.find((candidate) => candidate.id === sessionId);
      if (!session) return;
      try {
        await orchestrator.parkSession(session);
      } catch (error) {
        showSnackbar(`Failed to close session: ${error instanceof Error ? error.message : String(error)}`, "error");
      }
      return;
    }

    // Diff and editor tabs — just remove from tree
    if (activeEntry.type === "diff" || activeEntry.type === "editor") {
      splitTree.removeSessionFromLeaf(activeEntry.ptyKey);
      tick().then(() => refocusTerminal());
      return;
    }

    await closeShellTabInTree(activeEntry.ptyKey);
  }

  /** Close a shell tab using its PTY key rather than a visual tab position. */
  async function closeShellTabInTree(ptyKey: string): Promise<void> {
    if (pendingShellCommands.has(ptyKey)) {
      showSnackbar("Terminal editor is still starting", "error");
      return;
    }

    const colonIdx = ptyKey.lastIndexOf(":");
    if (colonIdx === -1) return;
    const sessionId = ptyKey.slice(0, colonIdx);
    const tabIndex = parseInt(ptyKey.slice(colonIdx + 1), 10);
    if (isNaN(tabIndex)) return;

    try {
      // Keep the pane intact until the orchestrator confirms the backend close.
      // This avoids an orphaned terminal UI when the daemon close request fails.
      await orchestrator.closeShellTab(sessionId, tabIndex);
      splitTree.removeSessionFromLeaf(ptyKey);
      await tick();
      refocusTerminal();
    } catch (error) {
      showSnackbar(`Failed to close shell tab: ${error instanceof Error ? error.message : String(error)}`, "error");
    }
  }

  function preserveKeyboardSelectedTerminal(entry: import("./lib/split-tree.svelte").TabEntry): void {
    if (entry.type !== "agent" && entry.type !== "shell") return;
    const sessionId = ptyKeyToSessionId(entry.ptyKey);
    if (sessionId !== activeSessionId) selectWorkspaceSession(sessionId);
    // Session synchronization can focus the agent tab. Restore the specific
    // keyboard-selected PTY after that reactive update settles.
    tick().then(() => requestAnimationFrame(() => {
      splitTree.focusTab(entry.ptyKey);
      requestTerminalFocus(entry.ptyKey);
    }));
  }

  /** Navigate to the next tab in the focused split leaf. */
  function splitNextTab(): void {
    const leaf = splitTree.getFocusedLeaf();
    if (!leaf || leaf.tabs.length <= 1) return;
    const currentIdx = leaf.tabs.findIndex((t) => t.ptyKey === leaf.activeTab);
    const next = leaf.tabs[(currentIdx + 1) % leaf.tabs.length];
    splitTree.setLeafActiveTab(leaf.id, next.ptyKey);
    preserveKeyboardSelectedTerminal(next);
  }

  /** Navigate to the previous tab in the focused split leaf. */
  function splitPrevTab(): void {
    const leaf = splitTree.getFocusedLeaf();
    if (!leaf || leaf.tabs.length <= 1) return;
    const currentIdx = leaf.tabs.findIndex((t) => t.ptyKey === leaf.activeTab);
    const previous = leaf.tabs[(currentIdx - 1 + leaf.tabs.length) % leaf.tabs.length];
    splitTree.setLeafActiveTab(leaf.id, previous.ptyKey);
    preserveKeyboardSelectedTerminal(previous);
  }

  /** Toggle diff tab: if it exists in the tree, focus it; otherwise add it to focused leaf. */
  function toggleDiffInTree(): void {
    if (!activeSessionId) return;
    if (toggleDiffResource(activeSessionId) === "closed") {
      tick().then(() => refocusTerminal());
    }
  }

  function confirmPendingShellCommandStarted(ptyKey: string): void {
    consumePendingTerminalEditorCommand({ ptyKey, pendingCommands: pendingShellCommands });
  }

  async function openTerminalEditorInTree(sessionId: string, filePath: string): Promise<void> {
    try {
      const opened = await queueTerminalEditor({
        sessionId,
        filePath,
        getTerminalCommand: editorApi.getTerminalCommand,
        getFocusedLeafId: splitTree.getFocusedLeafId,
        addTab,
        removeTab,
        incrementTabCount: pty.incrementTabCount,
        closeTab: pty.closeTab,
        addShellTab: addShellTabToLeaf,
        pendingCommands: pendingShellCommands,
      });
      if (opened) tick().then(() => requestAnimationFrame(() => refocusTerminal()));
    } catch (error) {
      showSnackbar(`Failed to open terminal editor: ${error}`, "error");
    }
  }

  async function handleShellAttachError(ptyKey: string, error: unknown): Promise<void> {
    try {
      const rolledBack = await rollbackPendingTerminalEditor({
        ptyKey,
        pendingCommands: pendingShellCommands,
        removeTab,
        closeTab: pty.closeTab,
      });
      if (rolledBack) splitTree.removeSessionFromLeaf(ptyKey);
    } catch (rollbackError) {
      console.warn("Failed to clean up terminal editor tab", rollbackError);
    }
    showSnackbar(`Failed to open terminal editor: ${error}`, "error");
  }

  /** Open a file with the globally configured editor. */
  async function openFileInTree(sessionId: string, filePath: string): Promise<void> {
    if (filePath.split(/[/\\]/).includes("..")) return;
    const result = await openFileWithConfiguredEditor(getSettings().editor, {
      openEmbedded: () => { openEditorResource(sessionId, filePath); },
      openTerminal: () => openTerminalEditorInTree(sessionId, filePath),
      openExternal: async () => {
        try {
          await editorApi.openExternal(sessionId, filePath);
        } catch (error) {
          showSnackbar(`Failed to open external editor: ${error}`, "error");
        }
      },
    });
    if (result === "invalid") showSnackbar("Editor configuration has an unknown mode", "error");
  }

  // Sync terminal selections—not editor/diff tabs—to the focused agent session.
  function syncFocusedLeafToOrchestrator(): void {
    const leaf = splitTree.getFocusedLeaf();
    const activeEntry = leaf ? splitTree.getActiveTabEntry(leaf) : null;
    if (!activeEntry || (activeEntry.type !== "agent" && activeEntry.type !== "shell")) return;
    const sessionId = ptyKeyToSessionId(activeEntry.ptyKey);
    if (sessionId !== activeSessionId) selectWorkspaceSession(sessionId);
  }

  // ─── Split tree persistence (DB) ────────────────────────────────────────────
  let splitSaveTimeout: ReturnType<typeof setTimeout> | null = null;

  function saveSplitTreeToDb(): void {
    const data = splitTree.serialize();
    const workspace = lastTreeWorkspace;
    if (!data || !workspace) return;
    const layoutJson = JSON.stringify(data);
    const save = workspace.taskKey
      ? sessionsApi.saveTaskWorkspaceLayout(workspace.projectId, workspace.taskKey, layoutJson)
      : sessionsApi.saveLayout(workspace.key.slice("session:".length), layoutJson);
    save.catch((e) => console.warn("Failed to save workspace layout", workspace.key, e));
  }

  // Auto-save split tree on changes (debounced 500ms)
  $effect(() => {
    const _tree = splitTree.getTree();
    if (splitTreeInitialized && _tree && lastTreeWorkspace && !loadingLayout) {
      if (splitSaveTimeout) clearTimeout(splitSaveTimeout);
      splitSaveTimeout = setTimeout(saveSplitTreeToDb, 500);
    }
    return () => { if (splitSaveTimeout) clearTimeout(splitSaveTimeout); };
  });

  // ─── Project management ─────────────────────────────────────────────────────
  async function openPreferences() {
    const existing = await WebviewWindow.getByLabel("preferences");
    if (existing) { existing.setFocus(); return; }
    new WebviewWindow("preferences", { url: "index.html?page=preferences", title: "Preferences", width: 720, height: 680, parent: getCurrentWindow(), resizable: true, minimizable: false, maximizable: false });
  }

  async function doRename(id: string, name: string) {
    await sessionsApi.rename(id, name);
    orchestrator.updateSessionName(id, name);
    renamingSessionId = null;
    focusTerminal();
  }

  function openAddProject() {
    projectToEdit = null;
    showProjectForm = true;
  }

  function openEditProject(project: Project) {
    projectToEdit = project;
    showProjectForm = true;
  }

  async function finishProjectForm() {
    showProjectForm = false;
    projectToEdit = null;
    await taskStore.refresh(projects.map((project) => project.path));
    focusTerminal();
  }

  function cancelProjectForm() {
    showProjectForm = false;
    projectToEdit = null;
    tick().then(() => refocusTerminal());
  }

  async function deleteProject(p: Project) {
    await projectStore.deleteProject(p.id);
    projectToDelete = null;
  }

  /** Delete a loop record only (sessions become standalone). */
  async function deleteLoopOnly(loopId: string) {
    await loopsApi.delete(loopId);
    if (loopStore.getActiveLoopId() === loopId) loopStore.setActiveLoopId(null);
    loopStore.refreshAllLoops(projects.map(p => p.id));
  }

  /** Delete a loop and destroy all its linked sessions. */
  async function deleteLoopAndSessions(loopId: string) {
    const sessionIds = await loopsApi.delete(loopId);
    if (loopStore.getActiveLoopId() === loopId) loopStore.setActiveLoopId(null);
    for (const sid of sessionIds) {
      const s = sessions.find(x => x.id === sid);
      if (s) await orchestrator.deleteSession(s);
    }
    loopStore.refreshAllLoops(projects.map(p => p.id));
  }

  // ─── Plugin workspace ─────────────────────────────────────────────────────

  async function refreshPlugins(): Promise<boolean> {
    try {
      pluginInventory = await pluginsApi.list();
      const actionRevision = pluginSessionActionsRevision;
      const actions = await pluginsApi.listSessionActions();
      if (actionRevision === pluginSessionActionsRevision) {
        pluginSessionActions = actions;
      }
      return true;
    } catch (error) {
      console.warn("Failed to load plugin inventory", error);
      return false;
    }
  }

  function leavePluginWorkspace(): void {
    activePluginId = null;
    activeContributionId = null;
  }

  function closePluginContributionModal(): void {
    modalPluginId = null;
    modalContributionId = null;
    tick().then(() => refocusTerminal());
  }

  function openPluginContributionModal(pluginId: string, contributionId: string): void {
    const plugin = pluginInventory.find((candidate) => candidate.id === pluginId && candidate.state === "running");
    const contribution = plugin?.ui_contributions.find((candidate) =>
      candidate.id === contributionId && candidate.placement === "session.panel",
    );
    if (!plugin || !contribution || !activeSession) {
      showSnackbar("Plugin contribution is unavailable for the selected session");
      return;
    }
    leavePluginWorkspace();
    modalPluginId = pluginId;
    modalContributionId = contributionId;
  }

  function openTitlebarPluginContribution(pluginId: string, contributionId: string): void {
    const contribution = pluginInventory
      .find((candidate) => candidate.id === pluginId && candidate.state === "running")
      ?.ui_contributions.find((candidate) => candidate.id === contributionId);
    if (contribution?.placement === "session.panel") {
      openPluginContributionModal(pluginId, contributionId);
      return;
    }
    openPluginContribution(pluginId, contributionId);
  }

  function focusPluginInteraction(): boolean {
    const interaction = document.querySelector<HTMLElement>("[data-plugin-interaction-host] [data-plugin-ui-contribution]");
    if (!interaction) return false;
    interaction.focus();
    return true;
  }

  function invalidatePluginPage(pluginId: string): void {
    if (activePluginId === pluginId) leavePluginWorkspace();
  }

  async function runPluginSessionAction(session: Session, action: PluginSessionAction): Promise<void> {
    try {
      await pluginsApi.call(action.plugin_id, "plugin.sessionAction", {
        action_id: action.id,
        session_id: session.id,
      });
    } catch (error) {
      showSnackbar(`Integration action failed: ${String(error)}`);
    }
  }

  function showIntegrationCompletionPrompt(session: Session, message: string): void {
    showMergePrompt({
      sessionId: session.id,
      sessionName: session.name || session.branch,
      taskKey: session.task_key,
      message,
      onArchive: (id) => {
        const found = sessions.find((candidate) => candidate.id === id);
        return found ? orchestrator.archiveSession(found) : Promise.resolve();
      },
      onDestroy: (id) => {
        const found = sessions.find((candidate) => candidate.id === id);
        return found ? orchestrator.deleteSession(found) : Promise.resolve();
      },
      onTaskDone: session.task_key
        ? async (id) => {
            const found = sessions.find((candidate) => candidate.id === id);
            if (!found?.task_key) return;
            const project = projects.find((candidate) => candidate.id === found.project_id);
            if (project) await taskStore.moveTask(found.task_key, "done", project.path);
          }
        : undefined,
    });
  }

  function openPluginContribution(pluginId: string, contributionId: string): void {
    const plugin = pluginInventory.find((candidate) => candidate.id === pluginId && candidate.state === "running");
    const contribution = plugin?.ui_contributions.find((candidate) =>
      candidate.id === contributionId && ["main-pane", "session.panel"].includes(candidate.placement),
    );
    if (!plugin || !contribution || (contribution.placement === "session.panel" && !activeSession)) {
      showSnackbar("Plugin contribution is unavailable for the selected session");
      return;
    }
    if (activePluginId === pluginId && activeContributionId === contributionId) return;
    loopStore.setActiveLoopId(null);
    activePluginId = pluginId;
    activeContributionId = contributionId;
  }

  function selectWorkspaceSession(sessionId: string): void {
    const session = sessions.find((candidate) => candidate.id === sessionId);
    if (session?.task_key) {
      const project = projects.find((candidate) => candidate.id === session.project_id);
      const task = project ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === session.task_key) : undefined;
      if (project && task) {
        selectedTaskWorkspace = { project, task };
        touchWorkspaceMru(toTaskWorkspaceId(project.id, task.key));
      }
    } else {
      selectedTaskWorkspace = null;
    }
    leavePluginWorkspace();
    loopStore.setActiveLoopId(null);
    orchestrator.selectSession(sessionId);
    requestTerminalFocus(sessionId);
  }

  function openSessionForTask(task: TaskItem, project: Project): void {
    taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: task.title, prompt: "", baseBranch: task.base_branch, projectId: project.id };
    showSessionForm = true;
  }

  function selectWorkspaceTask(task: TaskItem, repoPath: string): void {
    const project = projects.find((candidate) => candidate.path === repoPath);
    if (!project) return;
    selectedTaskWorkspace = { task, project };
    touchWorkspaceMru(toTaskWorkspaceId(project.id, task.key));
    loopStore.setActiveLoopId(null);
    const linked = sessions.find((session) => session.project_id === project.id && session.task_key === task.key);
    if (linked) {
      const alreadyActive = linked.id === activeSessionId;
      selectWorkspaceSession(linked.id);
      // The active session ID does not change when returning from an empty
      // TaskWorkspace. Reload its layout because the empty workspace reset the tree.
      if (alreadyActive) {
        const workspace = workspaceForSession(linked.id);
        if (workspace) void loadLayoutForWorkspace(workspace, linked.id);
      }
      // Keep the routed task authoritative even if session/task-store reconciliation lags.
      selectedTaskWorkspace = { task, project };
    } else splitTree.resetTree();
  }

  async function restoreTaskSession(session: Session): Promise<void> {
    await sessionsApi.restore(session.id);
    await orchestrator.loadSessions();
    selectWorkspaceSession(session.id);
  }

  $effect(() => {
    const workspace = activeTaskWorkspace;
    if (!workspace) { archivedTaskSessions = []; return; }
    sessionsApi.listArchived().then((items) => {
      if (activeTaskWorkspace?.task.key === workspace.task.key && activeTaskWorkspace.project.id === workspace.project.id) {
        archivedTaskSessions = items.filter((session) => session.project_id === workspace.project.id && session.task_key === workspace.task.key);
      }
    }).catch(() => { archivedTaskSessions = []; });
  });

  function jumpToWorkspaceSession(index: number): void {
    leavePluginWorkspace();
    loopStore.setActiveLoopId(null);
    orchestrator.jumpToSession(index);
  }

  function selectWorkspaceLoop(loopId: string): void {
    leavePluginWorkspace();
    loopStore.setActiveLoopId(loopId);
    touchWorkspaceMru(`loop:${loopId}`);
  }

  /** Promote a UI-only workspace identity without sending it to session MRU persistence. */
  function touchWorkspaceMru(id: string): void {
    workspaceMru = [id, ...workspaceMru.filter((candidate) => candidate !== id)];
  }

  /** Runtime switcher candidates: collapse task sessions to their task workspace. */
  function getWorkspaceMruCandidates(): string[] {
    const valid = getSwitchableIds();
    const persisted = getMruList().map((id) => {
      const session = sessions.find((candidate) => candidate.id === id);
      return session?.task_key ? toTaskWorkspaceId(session.project_id, session.task_key) : id;
    });
    return [...workspaceMru, ...persisted, ...sidebarSessionOrder]
      .filter((id, index, all) => valid.has(id) && all.indexOf(id) === index);
  }

  // ─── Lifecycle ──────────────────────────────────────────────────────────────

  function requestTerminalFocus(sessionId: string): void {
    tick().then(() => {
      requestAnimationFrame(() => {
        terminalFocusRequest = { id: (terminalFocusRequest?.id ?? 0) + 1, sessionId };
      });
    });
  }

  function requestFocusedTerminalFocus(): void {
    const leaf = splitTree.getFocusedLeaf();
    const activeTab = leaf ? splitTree.getActiveTabEntry(leaf) : null;
    if (activeTab?.type === "agent" || activeTab?.type === "shell") requestTerminalFocus(activeTab.ptyKey);
  }

  /** Valid IDs for MRU cycling — task workspaces, legacy sessions, and loops. */
  function getSwitchableIds(): Set<string> {
    const ids = new Set<string>(sidebarSessionOrder.filter((id) => {
      const taskWorkspace = parseTaskWorkspaceId(id);
      if (!taskWorkspace) return true;
      const project = projects.find((candidate) => candidate.id === taskWorkspace.projectId);
      const task = project
        ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === taskWorkspace.taskKey)
        : undefined;
      return task?.status !== "todo";
    }));
    for (const session of sessions) {
      if (!session.task_key) ids.add(session.id);
    }
    for (const p of projects) {
      for (const loop of loopStore.getLoopsForProject(p.id)) {
        if (loop.status !== "draft" && !isTerminal(loop.status)) ids.add(`loop:${loop.id}`);
      }
    }
    return ids;
  }

  onMount(() => {
    projectStore.loadProjects().then(() => {
      taskStore.loadTasks(projectStore.getProjects().map((p) => p.path));
      loopStore.refreshAllLoops(projectStore.getProjects().map((p) => p.id));
    });
    orchestrator.loadSessions();
    loadSettings().then(() => loadTheme());

    const cleanupEvents = orchestrator.startEventListeners();
    const cleanupSymphony = orchestrator.startSymphonyPolling();
    const cleanupLoopListener = loopStore.startLoopEventListener(() => projectStore.getProjects().map((p) => p.id));
    const unlistenSettings = listen("settings-changed", () => { loadSettings().then(() => loadTheme()); });
    const unlistenCleanup = listen<string>("cleanup-error", (event) => { showSnackbar(event.payload); });
    const unlistenShellPtyExit = listen<{ pty_key: string }>("pty-exited", (event) => {
      const ptyKey = event.payload.pty_key;
      const separator = ptyKey.lastIndexOf(":");
      if (separator === -1) return;

      // An explicit close removes the tab before daemon shutdown. Ignore the
      // resulting pty-exited event rather than issuing a second close_tab.
      if (!splitTree.findTab(ptyKey)) return;

      pendingShellCommands.delete(ptyKey);
      splitTree.removeSessionFromLeaf(ptyKey);
      const sessionId = ptyKey.slice(0, separator);
      const tabIndex = Number.parseInt(ptyKey.slice(separator + 1), 10);
      if (!sessionId || Number.isNaN(tabIndex)) return;
      void orchestrator.closeShellTab(sessionId, tabIndex).catch((error) => {
        showSnackbar(`Failed to finalize shell tab close: ${error instanceof Error ? error.message : String(error)}`, "error");
      });
    });
    const unlistenPluginRuntime = listen<import("./lib/types").PluginInventory>("plugin-runtime-changed", (event) => {
      pluginSessionActionsRevision += 1;
      pluginInventory = pluginInventory.filter((plugin) => plugin.id !== event.payload.id).concat(event.payload);
      if (event.payload.state !== "running") {
        pluginSessionActions = pluginSessionActions.filter((action) => action.plugin_id !== event.payload.id);
        invalidatePluginPage(event.payload.id);
      }
    });
    const unlistenPluginActions = listen<{ plugin_id: string; actions: PluginSessionAction[] }>("plugin-session-actions", (event) => {
      pluginSessionActionsRevision += 1;
      const { plugin_id: pluginId, actions } = event.payload;
      if (!pluginInventory.some((plugin) => plugin.id === pluginId)) return;
      pluginSessionActions = pluginSessionActions
        .filter((action) => action.plugin_id !== pluginId)
        .concat(actions.map((action) => ({ ...action, plugin_id: pluginId })));
    });
    const unlistenPluginAdvisory = listen<{ plugin_id: string; advisory: PluginSessionAdvisory }>("plugin-session-advisory", (event) => {
      const { plugin_id: pluginId, advisory } = event.payload;
      if (!sessions.some((session) => session.id === advisory.session_id)) return;
      const pluginName = pluginInventory.find((plugin) => plugin.id === pluginId)?.name ?? pluginId;
      showSnackbar(`${pluginName}: ${advisory.message}`, advisory.severity);
    });
    const unlistenPluginCompletion = listen<{ plugin_id: string; completion: PluginSessionCompletion }>("plugin-session-completed", (event) => {
      const session = sessions.find((candidate) => candidate.id === event.payload.completion.session_id);
      if (!session) return;
      const pluginName = pluginInventory.find((plugin) => plugin.id === event.payload.plugin_id)?.name ?? event.payload.plugin_id;
      showIntegrationCompletionPrompt(session, event.payload.completion.message ?? `${pluginName} completed`);
    });

    void initUpdateListener().then(async () => {
      try {
        const update = await updater.getPending();
        if (update) setLaunchUpdateAvailable(update);
      } catch (error) {
        console.warn("Failed to load pending update:", error);
      }
    });
    const onPluginShortcut = (event: KeyboardEvent) => {
      // This capture listener runs before the host router. Defer plugin routing
      // until propagation completes so a built-in shortcut always wins.
      queueMicrotask(() => {
      if (event.defaultPrevented) return;
      const target = findPluginShortcut(event, sessionPanelCommands, mainPaneCommands);
      if (!target) return;
      event.preventDefault();
      if (target.contribution.placement === "session.panel") {
        if (modalPluginId === target.plugin.id && modalContributionId === target.contribution.id) {
          closePluginContributionModal();
          return;
        }
        openPluginContributionModal(target.plugin.id, target.contribution.id);
        return;
      }
      openPluginContribution(target.plugin.id, target.contribution.id);
      });
    };
    window.addEventListener("keydown", onPluginShortcut, true);
    let pluginListenersDisposed = false;
    const pluginListenerReady = Promise.all([
      unlistenPluginRuntime,
      unlistenPluginActions,
      unlistenPluginAdvisory,
      unlistenPluginCompletion,
    ]);
    void pluginListenerReady
      .then(() => {
        if (!pluginListenersDisposed) void refreshPlugins();
      })
      .catch((error) => {
        console.warn("Failed to register plugin event listeners", error);
        if (!pluginListenersDisposed) void refreshPlugins();
      });

    notify.isInstalled().then((installed) => { if (!installed) showHookPrompt = true; });
    sessionLogs.isEnabled().then((enabled) => { logViewerEnabled = enabled; });
    const unlistenClose = orchestrator.setupQuitGuard((count) => { quitDirectCount = count; showQuitConfirm = true; });

    const cleanup = installKeyboardRouter(
      (action) => {
        if (action.type === "new_session") {
          showNewItemModal = true;
        } else if (action.type === "new_project") { openAddProject(); }
        else if (action.type === "toggle_sidebar") { sidebarVisible = !sidebarVisible; if (sidebarVisible) focusSidebar(); else focusTerminal(); }
        else if (action.type === "jump_to_session") { jumpToWorkspaceSession(action.index); }
        else if (action.type === "tab_switch") {
          const sw = getCycleState();
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeTaskWorkspace ? toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key) : activeSessionId ?? undefined;
          if (!sw.isCycling) startCycle(currentId, getSwitchableIds(), getWorkspaceMruCandidates());
          else advance(1);
        } else if (action.type === "tab_switch_reverse") {
          const sw = getCycleState();
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeTaskWorkspace ? toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key) : activeSessionId ?? undefined;
          if (!sw.isCycling) { startCycle(currentId, getSwitchableIds(), getWorkspaceMruCandidates()); advance(-1); }
          else advance(-1);
        } else if (action.type === "focus_terminal") {
          if (getCycleState().isCycling) cancel();
          if (navCycle.isCycling()) navCycle.cancel();
          showSessionForm = false; showProjectForm = false; projectToEdit = null; showShortcuts = false; showNewItemModal = false; showTaskForm = false; closePluginContributionModal(); showLoopForm = false; sessionToDelete = null; commandMenuOpen = false; commandMenuFileMode = false;
        } else if (action.type === "command_palette") { commandMenuOpen = !commandMenuOpen; }
        else if (action.type === "open_preferences") { openPreferences(); }
        else if (action.type === "show_shortcuts") { showShortcuts = !showShortcuts; }
        else if (action.type === "new_tab") { splitNewTab(); }
        else if (action.type === "close_tab") { splitCloseTab(); }
        else if (action.type === "next_tab") { splitNextTab(); }
        else if (action.type === "prev_tab") { splitPrevTab(); }
        else if (action.type === "next_session") {
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeTaskWorkspace ? toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key) : activeSessionId ?? undefined;
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, currentId, 1);
          else navCycle.advance(1);
        } else if (action.type === "prev_session") {
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeTaskWorkspace ? toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key) : activeSessionId ?? undefined;
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, currentId, -1);
          else navCycle.advance(-1);
        }
        else if (action.type === "toggle_diff") { toggleDiffInTree(); }
        else if (action.type === "focus_file_explorer") {
          const wasExplorerFocused = getActiveZone() === "explorer";
          toggleExplorerFocus();
          if (!wasExplorerFocused) fileExplorerVisible = true;
        }
        else if (action.type === "toggle_file_explorer") {
          fileExplorerVisible = !fileExplorerVisible;
          if (fileExplorerVisible) focusExplorer();
          else if (getActiveZone() === "explorer") toggleExplorerFocus();
        }
        else if (action.type === "toggle_task_panel") { if (!sidebarVisible) sidebarVisible = true; }
        else if (action.type === "toggle_sessions_panel") { if (!sidebarVisible) sidebarVisible = true; }
        else if (action.type === "refresh_tasks") { if (!sidebarVisible) sidebarVisible = true; taskStore.refresh(projects.map((p) => p.path)); }
        else if (action.type === "open_file") { commandMenuFileMode = true; commandMenuOpen = true; }
        else if (action.type === "save_file") { saveActiveEditorResource(); }
        else if (action.type === "focus_merge_prompt") {
          if (getPrompt()) focusMergePrompt();
          else {
            const u = getUpdateState();
            if (u.updateAvailable && !u.dismissed) focusUpdateToast();
            else focusPluginInteraction();
          }
        }
        else if (action.type === "split_vertical" || action.type === "split_horizontal" || action.type === "close_split" || action.type === "focus_split_left" || action.type === "focus_split_right" || action.type === "focus_split_up" || action.type === "focus_split_down" || action.type === "move_tab_left" || action.type === "move_tab_right" || action.type === "move_tab_up" || action.type === "move_tab_down") { handleSplitAction(action.type); }
      },
      () => !showSessionForm && !showProjectForm && !commandMenuOpen && !showShortcuts && !showNewItemModal && !showTaskForm && !modalPluginId && !showLoopForm && !getCycleState().isCycling && !navCycle.isCycling(),
      () => {
        const leaf = splitTree.getFocusedLeaf();
        return getActiveZone() === "editor" && !!leaf && splitTree.getActiveTabEntry(leaf)?.type === "editor";
      },
      () => !!document.activeElement?.closest('[data-form-keyboard]'),
    );

    function onModalKeydown(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (showNewItemModal) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape') { showNewItemModal = false; }
        else if (e.key === 's') { showNewItemModal = false; taskPrefill = null; showSessionForm = true; }
        else if (e.key === 't') { showNewItemModal = false; showTaskForm = true; }
        else if (e.key === 'l') { showNewItemModal = false; showLoopForm = true; }
      } else if (sessionToDelete) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape' || e.key === 'c' || e.key === 'n') sessionToDelete = null;
        else if (e.key === 'd' || e.key === 'y') { const s = sessionToDelete; sessionToDelete = null; if (s) orchestrator.deleteSession(s); }
      } else if (projectToDelete) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape' || e.key === 'c' || e.key === 'n') projectToDelete = null;
        else if (e.key === 'd' || e.key === 'y') deleteProject(projectToDelete);
      } else if (loopToDelete) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape' || e.key === 'c' || e.key === 'n') { loopToDelete = null; focusTerminal(); }
        else if (e.key === 'l') { const id = loopToDelete.id; loopToDelete = null; focusTerminal(); deleteLoopOnly(id); }
        else if (e.key === 'a') { const id = loopToDelete.id; loopToDelete = null; focusTerminal(); deleteLoopAndSessions(id); }
      } else if (showQuitConfirm) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape' || e.key === 'n') showQuitConfirm = false;
        else if (e.key === 'q' || e.key === 'y') { showQuitConfirm = false; getCurrentWindow().destroy(); }
      }
    }
    window.addEventListener("keydown", onModalKeydown, true);

    /** Route a navigation target (task workspace, session, or loop). */
    function routeNavTarget(target: string): void {
      if (isLoopId(target)) {
        selectWorkspaceLoop(parseLoopId(target));
      } else if (isTaskWorkspaceId(target)) {
        const parsed = parseTaskWorkspaceId(target);
        const project = parsed ? projects.find((candidate) => candidate.id === parsed.projectId) : undefined;
        const task = project && parsed ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === parsed.taskKey) : undefined;
        if (project && task) selectWorkspaceTask(task, project.path);
      } else {
        selectWorkspaceSession(target);
      }
    }

    function onKeyUp(e: KeyboardEvent) {
      const isModRelease = (e.key === "Control" && !e.ctrlKey) || (e.key === "Meta" && !e.metaKey);
      if (!isModRelease) return;
      if (getCycleState().isCycling) {
        const target = commit();
        if (target && !isLoopId(target)) {
          routeNavTarget(target);
          focusTerminal();
        } else if (target) routeNavTarget(target);
      }
      if (navCycle.isCycling()) {
        const target = navCycle.commit();
        if (target && !isLoopId(target)) {
          routeNavTarget(target);
          focusTerminal();
        } else if (target) routeNavTarget(target);
      }
    }
    function onBlur() { setTimeout(() => { if (!document.hasFocus()) { if (getCycleState().isCycling) cancel(); if (navCycle.isCycling()) navCycle.cancel(); } }, 0); }
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);

    return () => { pluginListenersDisposed = true; window.removeEventListener("keydown", onPluginShortcut, true); cleanup(); cleanupEvents(); cleanupSymphony(); cleanupLoopListener(); unlistenSettings.then((fn) => fn()); unlistenCleanup.then((fn) => fn()); unlistenShellPtyExit.then((fn) => fn()); unlistenPluginRuntime.then((fn) => fn()); unlistenPluginActions.then((fn) => fn()); unlistenPluginAdvisory.then((fn) => fn()); unlistenPluginCompletion.then((fn) => fn()); unlistenClose.then((fn) => fn()); window.removeEventListener("keydown", onModalKeydown, true); window.removeEventListener("keyup", onKeyUp); window.removeEventListener("blur", onBlur); };
  });
</script>

<main class="flex flex-col h-screen">
  <Titlebar
    projectName={activeProjectName}
    sessionName={activeSessionName}
    {sidebarVisible}
    sessionId={activeSessionId}
    tabs={isEmptyTaskWorkspace || hasMultiplePanes ? [] : titlebarTabs}
    activeTabIndex={titlebarActiveTabIdx}
    activeTabId={titlebarActiveTabId}
    runningCount={sessions.filter(s => s.status === 'active').length}
    activeProvider={activeSession?.provider ?? null}
    onSelectTab={(i, tabId) => {
      leavePluginWorkspace();
      loopStore.setActiveLoopId(null);
      const tree = splitTree.getTree();
      if (tree?.type === "leaf") {
        const entry = tree.tabs.find((candidate) => candidate.ptyKey === tabId)
          ?? tree.tabs.find((candidate, visualIndex) => tabIndexForEntry(candidate, visualIndex) === i);
        if (entry) {
          splitTree.setLeafActiveTab(tree.id, entry.ptyKey);
          if (entry.type === "agent" || entry.type === "shell") selectWorkspaceSession(ptyKeyToSessionId(entry.ptyKey));
        }
      } else orchestrator.selectUnifiedTab(i);
    }}
    onCloseTab={(i) => {
      if (!activeSessionId) return;
      if (i === -1) orchestrator.closeDiffTab(activeSessionId);
      else if (i === -2) orchestrator.closeEditorTab(activeSessionId);
      else {
        const tab = titlebarTabs.find((entry) => entry.index === i);
        if (tab) closeShellTabInTree(`${activeSessionId}:${tab.index}`);
      }
    }}
    onAddTab={() => orchestrator.handleNewTab()}
    {titlebarContributions}
    titlebarSession={activePluginSessionContext}
    onOpenTitlebarContribution={openTitlebarPluginContribution}
    onOpenCommand={() => { commandMenuFileMode = false; commandMenuOpen = true; }}
    {symphonyStatus}
  />

  <div class="flex flex-1 min-h-0">
  {#if sidebarVisible}
      <UnifiedSidebar
        {renamingSessionId}
        onSelectSession={selectWorkspaceSession}
        onArchiveSession={(s) => orchestrator.archiveSession(s)}
        onDeleteSession={(s) => (sessionToDelete = s)}
        onRestartSession={(s) => orchestrator.restartSession(s)}
        onRenameSession={doRename}
        onStartRename={(id) => { renamingSessionId = id || null; if (!id) focusTerminal(); }}
        onDeleteProject={(p) => (projectToDelete = p)}
        onEditProject={openEditProject}
        onPickTask={(task, repoPath) => { const proj = projects.find(p => p.path === repoPath); if (proj) openSessionForTask(task, proj); }}
        onSelectTask={selectWorkspaceTask}
        onAddProject={openAddProject}
        onOpenPreferences={openPreferences}
        onCreateSession={() => { taskPrefill = null; showSessionForm = true; }}
        onSessionsChanged={() => { orchestrator.loadSessions(); taskStore.refresh(projects.map((p) => p.path)); }}
        onSelectLoop={selectWorkspaceLoop}
        onStartLoop={(id) => { loopsApi.start(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onTickLoop={(id) => { loopsApi.tick(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onStopLoop={(id) => { loopsApi.stop(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onDeleteLoop={(id) => { const loop = projects.flatMap(p => loopStore.getLoopsForProject(p.id)).find(l => l.id === id); if (!loop) return; const hasSessions = (loopStore.getSessionsForLoop(id) ?? []).length > 0; if (hasSessions) { loopToDelete = loop; } else { deleteLoopOnly(id); } }}
        onDeleteLoopSession={(session, loopId) => { const loop = projects.flatMap(p => loopStore.getLoopsForProject(p.id)).find(l => l.id === loopId); if (loop && isLoopActive(loop.status)) { showSnackbar("Stop the loop before deleting its sessions"); } else { orchestrator.deleteSession(session); } }}
        selectedTaskWorkspace={activeTaskWorkspace ? { projectId: activeTaskWorkspace.project.id, taskKey: activeTaskWorkspace.task.key } : null}
        selectedLoopId={activeLoopId}
        onToggleDiff={toggleDiffInTree}
        pluginContributions={sidebarPluginContributions}
        {sessionIndicatorContributions}
        pluginSessionActions={pluginSessionActions}
        onPluginSessionAction={runPluginSessionAction}
        onPluginNavigate={openPluginContribution}
        onPluginClose={leavePluginWorkspace}
      />
  {/if}

  <section class="flex-1 flex flex-col relative bg-main overflow-hidden">
    <div class="flex-1 relative overflow-hidden">
    {#if showProjectForm}
    <FormDialog title={projectToEdit ? "Edit Project" : "Add Project"} onClose={cancelProjectForm}>
      <ProjectForm project={projectToEdit} onCreated={finishProjectForm} onCancel={cancelProjectForm} />
    </FormDialog>
    {/if}

    {#if showSessionForm}
    <FormDialog title="New Session" onClose={() => { showSessionForm = false; taskPrefill = null; tick().then(() => refocusTerminal()); }}>
      <SessionForm
        {projects}
        {sessions}
        {taskPrefill}
        currentProjectId={taskPrefill?.projectId ?? sessions.find(s => s.id === activeSessionId)?.project_id ?? null}
        onCreateTask={() => {
          showSessionForm = false;
          taskPrefill = null;
          showTaskForm = true;
        }}
        onCreated={(session) => { leavePluginWorkspace(); showSessionForm = false; orchestrator.createSession(session); focusTerminal(); }}
        onCancel={() => { showSessionForm = false; taskPrefill = null; tick().then(() => refocusTerminal()); }}
      />
    </FormDialog>
    {/if}

    {#if showTaskForm}
    <FormDialog title="New Task" onClose={() => { showTaskForm = false; tick().then(() => refocusTerminal()); }}>
      <TaskForm
        mode={taskWorkspaceToEdit ? "edit" : "create"}
        {projects}
        {sessions}
        tasks={taskStore.getAllTasks()}
        initial={taskWorkspaceToEdit ? {
          key: taskWorkspaceToEdit.task.key,
          title: taskWorkspaceToEdit.task.title,
          description: taskWorkspaceToEdit.task.description,
          priority: taskWorkspaceToEdit.task.priority,
          parentKey: taskWorkspaceToEdit.task.parent_key,
          blockedBy: taskWorkspaceToEdit.task.blocked_by,
          tags: taskWorkspaceToEdit.task.tags,
          baseBranch: taskWorkspaceToEdit.task.base_branch,
          projectPath: taskWorkspaceToEdit.project.path,
        } : { projectPath: projects[0]?.path ?? "" }}
        onSubmitted={() => { taskWorkspaceToEdit = null; showTaskForm = false; taskStore.refresh(projects.map((p) => p.path)); focusTerminal(); }}
        onCancel={() => { taskWorkspaceToEdit = null; showTaskForm = false; tick().then(() => refocusTerminal()); }}
        onSessionCreated={(session) => { leavePluginWorkspace(); taskWorkspaceToEdit = null; showTaskForm = false; orchestrator.createSession(session); focusTerminal(); }}
      />
    </FormDialog>
    {/if}

    {#if showLoopForm}
    <FormDialog title="Start Loop" onClose={() => { showLoopForm = false; tick().then(() => refocusTerminal()); }}>
      <LoopForm
        projects={projects.map(p => ({ id: p.id, name: p.name, path: p.path }))}
        onCreated={(loop) => { showLoopForm = false; selectWorkspaceLoop(loop.id); loopStore.refreshAllLoops(projects.map(p => p.id)); focusTerminal(); }}
        onCancel={() => { showLoopForm = false; tick().then(() => refocusTerminal()); }}
      />
    </FormDialog>
    {/if}

    {#if getCycleState().isVisible}
      <TabSwitcher mruSessionIds={getCycleState().cycleList} selectedIndex={getCycleState().index} />
    {/if}

    <CommandMenu
      open={commandMenuOpen}
      openFileMode={commandMenuFileMode}
      onOpenChange={(v) => { commandMenuOpen = v; if (!v) commandMenuFileMode = false; }}
      onSelectSession={(id) => { selectWorkspaceSession(id); focusTerminal(); }}
      onArchiveSession={() => { if (activeSessionId) { const s = sessions.find(x => x.id === activeSessionId); if (s) orchestrator.archiveSession(s); } }}
      onDeleteSession={() => { if (activeSessionId) { const s = sessions.find(x => x.id === activeSessionId); if (s) sessionToDelete = s; } }}
      onRenameSession={() => { if (activeSessionId) { sidebarVisible = true; renamingSessionId = activeSessionId; } }}
      onRestoreSession={async (id) => { await sessionsApi.restore(id); await orchestrator.loadSessions(); }}
      onDestroyArchivedSession={async (id) => { await sessionsApi.destroy(id); }}
      onNewSession={() => { if (projects.length === 0) openAddProject(); else showSessionForm = true; }}
      onResetTerminal={() => {
        if (activeSessionId) {
          orchestrator.recordUserInput(activeSessionId);
          pty.write(activeSessionId, [0x0c]);
        }
      }}
      onArchiveProject={async (id) => { await projectStore.archiveProject(id); }}
      onHideProject={async (id) => { await projectStore.hideProject(id); }}
      onUnhideProject={async (id) => { await projectStore.unhideProject(id); }}
      onDeleteProject={(id) => { const p = projects.find(x => x.id === id); if (p) projectToDelete = p; }}
      onRestoreProject={async (id) => { await projectStore.restoreProject(id); }}
      onPickTask={(task) => { taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: task.title, prompt: "" }; showSessionForm = true; }}
      onCreateTask={() => { showTaskForm = true; }}
      onToggleDiff={() => toggleDiffInTree()}
      onOpenFile={(path) => { if (activeSessionId) openFileInTree(activeSessionId, path); }}
      onOpenLogViewer={logViewerEnabled ? () => { showLogViewer = true; } : undefined}
        onSplitVertical={() => handleSplitAction("split_vertical")}
      onSplitHorizontal={() => handleSplitAction("split_horizontal")}
      onCloseSplit={() => handleSplitAction("close_split")}
      pluginCommands={pluginCommands}
      onOpenPluginContribution={openPluginContribution}
    />

    <KeyboardShortcuts open={showShortcuts} onOpenChange={(v) => (showShortcuts = v)} />

    <!-- Split leaf snippet: renders a leaf pane with its own tab bar and terminal -->
    {#snippet splitLeafSnippet(leaf: LeafNode)}
      {@const leafTabs = getLeafTabInfo(leaf)}
      {@const activeEntry = splitTree.getActiveTabEntry(leaf)}
      {@const activeTabIdx = leafTabs.find((tab) => leaf.tabs.find((entry, visualIndex) => tabIndexForEntry(entry, visualIndex) === tab.index)?.ptyKey === leaf.activeTab)?.index ?? 0}
      {@const showLeafTabBar = hasMultiplePanes}
      <div
        class="split-leaf {hasMultiplePanes ? '' : 'split-leaf-single'}"
        class:split-leaf-focused={leaf.id === splitTree.getFocusedLeafId() && hasMultiplePanes}
        role="group"
        aria-label="Split pane"
        onclick={(event) => {
          splitTree.setFocusedLeaf(leaf.id);
          if (activeEntry && (activeEntry.type === "agent" || activeEntry.type === "shell")) {
            selectWorkspaceSession(ptyKeyToSessionId(activeEntry.ptyKey));
          }
          if (!(event.target instanceof Element && event.target.closest("[data-editor-tab]"))) {
            focusTerminal();
          }
        }}
      >
        {#if showLeafTabBar}
        <div class="flex items-stretch h-[38px] bg-chrome border-b border-border shrink-0">
          <TabStrip
            tabs={leafTabs}
            activeTabIndex={activeTabIdx >= 0 ? activeTabIdx : 0}
            activeTabId={leaf.activeTab}
            focused={leaf.id === splitTree.getFocusedLeafId()}
            showAddButton={true}
            showCloseButton={hasMultiplePanes}
            draggable={hasMultiplePanes}
            onSelectTab={(i, tabId) => {
              splitTree.setFocusedLeaf(leaf.id);
              const entry = leaf.tabs.find((tab) => tab.ptyKey === tabId)
                ?? leaf.tabs.find((tab, visualIndex) => tabIndexForEntry(tab, visualIndex) === i);
              if (!entry) return;
              splitTree.setLeafActiveTab(leaf.id, entry.ptyKey);
              if (entry.type === "agent" || entry.type === "shell") selectWorkspaceSession(ptyKeyToSessionId(entry.ptyKey));
            }}
            onAddTab={() => { splitTree.setFocusedLeaf(leaf.id); splitNewTab(); }}
            onClose={() => splitTree.closeSplit(leaf.id)}
            onTabDragStart={(e, tabIndex, tabId) => handleTabDragStart(e, tabId ?? leaf.tabs.find((tab, visualIndex) => tabIndexForEntry(tab, visualIndex) === tabIndex)?.ptyKey ?? "", leaf.id)}
            onTabDrop={(e, insertIndex) => handleTabDrop(e, leaf.id, insertIndex)}
            onTabDragOver={handleTabDragOver}
          />
        </div>
        {/if}
        <div class="split-leaf-content">
          {#each leaf.tabs as tabEntry (tabEntry.ptyKey)}
            {@const sessionId = ptyKeyToSessionId(tabEntry.ptyKey)}
            {@const session = sessions.find((s) => s.id === sessionId)}
            {@const isActiveInLeaf = tabEntry.ptyKey === leaf.activeTab}
            {@const project = session ? projects.find((p) => p.id === session.project_id) : null}
            {#if (tabEntry.type === "agent" || tabEntry.type === "shell")}
              {#if session && poolIsMounted(session.id)}
              <!-- Wrapper hides inactive tabs; Terminal's visible prop also pauses during loop overlay -->
              <div class="absolute inset-0" class:hidden={!isActiveInLeaf}>
                <Terminal
                  focusRequest={terminalFocusRequest}
                  sessionId={tabEntry.ptyKey}
                  visible={isActiveInLeaf && !activeLoopId && !activePluginId}
                  focused={isActiveInLeaf && sessionId === activeSessionId && !activePluginId && leaf.id === splitTree.getFocusedLeafId() && zone === "terminal" && !showNewItemModal && !sessionToDelete && !showTaskForm && !showProjectForm && !modalPluginId}
                  exited={tabEntry.type === "agent" && session.status === "exited"}
                  skipAttach={tabEntry.type === "shell"}
                  initialCommand={tabEntry.type === "shell" ? getPendingTerminalEditorCommand({ ptyKey: tabEntry.ptyKey, pendingCommands: pendingShellCommands }) : undefined}
                  onAttached={() => {
                    if (tabEntry.type === "shell") confirmPendingShellCommandStarted(tabEntry.ptyKey);
                    if (tabEntry.type === "agent" && session?.status === "exited") orchestrator.updateSessionStatus(session.id, "active");
                    if (tabEntry.type === "shell" && leaf.id === splitTree.getFocusedLeafId()) refocusTerminal();
                  }}
                  onAttachError={(error) => {
                    if (tabEntry.type === "shell") void handleShellAttachError(tabEntry.ptyKey, error);
                    else showSnackbar(String(error));
                  }}
                  onFocused={(event) => {
                    if (event.type === "focusin" && !isActiveInLeaf) return;
                    splitTree.setFocusedLeaf(leaf.id);
                    selectWorkspaceSession(sessionId);
                    focusTerminal();
                  }}
                  onUserInput={() => orchestrator.recordUserInput(sessionId)}
                />
              </div>
              {/if}
            {/if}
            <!-- Diff/Editor render when active; EditorTab stays mounted to preserve state -->
            {#if tabEntry.type === "diff"}
              {#if isActiveInLeaf}
                {#if session && project}
                  {@const repoPath = session.worktree_path ?? project.path}
                  {@const baseBranch = session.base_branch ?? "main"}
                  <ReviewTab
                    {repoPath}
                    {baseBranch}
                    visible={!activePluginId}
                    sessionId={sessionId}
                    onEditFile={(filePath) => openFileInTree(sessionId, filePath)}
                    onFileChange={(name) => splitTree.updateTabLabel(tabEntry.ptyKey, name)}
                  />
                {:else}
                  <div class="flex items-center justify-center h-full text-t3 text-sm" role="status">No project associated with this session</div>
                {/if}
              {/if}
            {/if}
            {#if tabEntry.type === "editor"}
              {#if session && project}
                {@const editorRepoPath = session.worktree_path ?? project.path}
                <EditorTab
                  repoPath={editorRepoPath}
                  sessionId={sessionId}
                  ptyKey={tabEntry.ptyKey}
                  sessionExited={session.status === "exited"}
                  visible={isActiveInLeaf && !activePluginId}
                  theme={isDark() ? "vs-dark" : "vs"}
                  initialFile={tabEntry.filePath}
                  focused={isActiveInLeaf && !activePluginId && leaf.id === splitTree.getFocusedLeafId() && zone === "editor"}
                  onClose={() => splitTree.removeSessionFromLeaf(tabEntry.ptyKey)}
                  onFocusEditor={() => { splitTree.focusTab(tabEntry.ptyKey); focusEditor(); }}
                  onFileChange={(name) => splitTree.updateTabLabel(tabEntry.ptyKey, name)}
                />
              {:else if isActiveInLeaf}
                <div class="flex items-center justify-center h-full text-t3 text-sm" role="status">No project associated with this session</div>
              {/if}
            {/if}
          {/each}
          {#if leaf.tabs.length === 0}
            <div class="flex items-center justify-center h-full text-t3 text-sm">
              <div class="text-center">
                <p>Empty pane</p>
                <p class="mt-1 text-xs">Press <kbd class="rounded border border-border px-1.5 py-0.5 text-xs font-mono">{MOD_LABEL}N</kbd> to create a session</p>
              </div>
            </div>
          {/if}
        </div>
      </div>
    {/snippet}

    {#if isEmptyTaskWorkspace && activeTaskWorkspace}
      <EmptyTaskWorkspace
        task={activeTaskWorkspace.task}
        archivedSessions={archivedTaskSessions}
        active={!activeLoopId && !activePluginId}
        onNewSession={() => openSessionForTask(activeTaskWorkspace.task, activeTaskWorkspace.project)}
        onRestore={restoreTaskSession}
        onEdit={() => { taskWorkspaceToEdit = activeTaskWorkspace; showTaskForm = true; }}
      />
    {/if}

    <!-- Always render through split tree (single leaf = normal view) -->
    {#if splitTreeNode && !isEmptyTaskWorkspace}
      <div class:hidden={!!activeLoopId || !!activePluginId} class="w-full h-full">
        <SplitContainer node={splitTreeNode} renderLeaf={splitLeafSnippet} />
      </div>
    {/if}

    {#if activePluginId}
      <div class="flex h-full flex-col bg-main">
        <div class="flex shrink-0 items-center gap-3 border-b border-border px-4 py-2">
          <button class="text-xs text-t2 hover:text-t1" onclick={leavePluginWorkspace}>← Back to workspace</button>
          <span class="text-sm font-medium text-t1">{activePlugin ? `${activePlugin.name} · ${activeContribution?.label ?? "Contribution"}` : "Plugin"}</span>
        </div>
        {#if activePlugin && activeContribution}
          <div class="min-h-0 flex-1"><PluginContributionHost plugin={activePlugin} contribution={activeContribution} session={activeContribution.placement === "session.panel" ? activePluginSessionContext : undefined} getFocusedAgentSession={() => activePluginSessionContext} onNavigate={openPluginContribution} onClose={leavePluginWorkspace} onOpenPreferences={openPreferences} autofocus /></div>
        {:else}
          <div class="flex min-h-0 flex-1 items-center justify-center text-sm text-t3">Plugin contribution is no longer available.</div>
        {/if}
      </div>
    {/if}

    {#if activeLoopId && !activePluginId}
      {@const loopProjectPath = (() => { const loops = projects.flatMap(p => loopStore.getLoopsForProject(p.id)); const loop = loops.find(l => l.id === activeLoopId); return loop ? (projects.find(p => p.id === loop.project_id)?.path ?? "") : ""; })()}
      <div class="w-full h-full bg-main">
        <LoopDashboard
          loopId={activeLoopId}
          projectPath={loopProjectPath}
          onSelectSession={selectWorkspaceSession}
          onOpenArtifact={(path) => {
            // If no active session, select the first session from this loop
            if (!activeSession) {
              const loopSessions = loopStore.getSessionsForLoop(activeLoopId);
              if (loopSessions.length > 0) {
                selectWorkspaceSession(loopSessions[0].session_id);
              } else return;
            }
            loopStore.setActiveLoopId(null);
            if (activeSessionId) openFileInTree(activeSessionId, path);
          }}
        />
      </div>
    {/if}

    {#if sessions.length === 0 && !activeTaskWorkspace && !showProjectForm && !showSessionForm && !activeLoopId && !activePluginId}
      <div class="flex items-center justify-center h-full">
        <p class="text-t2">No active session. Press <kbd class="rounded border border-border px-1.5 py-0.5 text-xs font-mono">{MOD_LABEL}N</kbd> to create one.</p>
      </div>
    {/if}

    {#if showHookPrompt}
      <div class="absolute top-2 left-4 right-4 z-20 flex items-center gap-3 rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-950 px-4 py-2.5 shadow-sm">
        <span class="text-sm text-amber-800 dark:text-amber-200">Install notification hook for instant agent-done alerts?</span>
        <button class="ml-auto rounded bg-amber-600 px-3 py-1 text-xs font-medium text-white hover:bg-amber-700" onclick={async () => { await notify.install(); showHookPrompt = false; }}>Install</button>
        <button class="rounded px-2 py-1 text-xs text-t3 hover:text-t1" onclick={() => (showHookPrompt = false)}>Dismiss</button>
      </div>
    {/if}

    {#if sessionToDelete}
      <SharedDialog open={true} onOpenChange={(v) => { if (!v) sessionToDelete = null; }} title="Delete session" class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden">
        <div>
          <div class="flex items-center px-[15px] pt-[13px] pb-[11px]">
            <span class="text-[13px] font-semibold text-t1">Delete <span class="font-bold">{sessionToDelete.name || sessionToDelete.branch}</span>?</span>
            <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
          </div>
          <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors" onclick={() => { sessionToDelete = null; }}>
              <span class="flex-1 text-[13.5px] text-t1">Cancel</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">n</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-red-500/10 transition-colors text-red-400" onclick={() => { const s = sessionToDelete; sessionToDelete = null; if (s) orchestrator.deleteSession(s); }}>
              <span class="flex-1 text-[13.5px]">Delete</span>
              <span class="font-mono text-[10px] text-red-400/70 border border-red-500/30 rounded-[5px] px-1.5 py-[2px]">d</span>
            </button>
          </div>
        </div>
      </SharedDialog>
    {/if}
    {#if projectToDelete}
      {@const ptd = projectToDelete}
      {@const projSessions = sessions.filter((s) => s.project_id === ptd.id)}
      {@const worktreeCount = projSessions.filter((s) => s.worktree_path).length}
      <SharedDialog open={true} onOpenChange={(v) => { if (!v) projectToDelete = null; }} title="Delete project" class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden">
        <div>
          <div class="flex flex-col px-[15px] pt-[13px] pb-[11px] gap-1">
            <div class="flex items-center">
              <span class="text-[13px] font-semibold text-t1">Delete <span class="font-bold">{ptd.name}</span>?</span>
              <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
            </div>
            <p class="text-[11px] text-t3">Removes {projSessions.length} session{projSessions.length !== 1 ? 's' : ''}{#if worktreeCount > 0} and {worktreeCount} worktree{worktreeCount !== 1 ? 's' : ''}{/if}. Cannot be undone.</p>
          </div>
          <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors" onclick={() => { projectToDelete = null; }}>
              <span class="flex-1 text-[13.5px] text-t1">Cancel</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">n</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-red-500/10 transition-colors text-red-400" onclick={() => { deleteProject(ptd); }}>
              <span class="flex-1 text-[13.5px]">Delete</span>
              <span class="font-mono text-[10px] text-red-400/70 border border-red-500/30 rounded-[5px] px-1.5 py-[2px]">d</span>
            </button>
          </div>
        </div>
      </SharedDialog>
    {/if}
    {#if loopToDelete}
      {@const ltd = loopToDelete}
      {@const loopSessionCount = (loopStore.getSessionsForLoop(ltd.id) ?? []).length}
      <SharedDialog open={true} onOpenChange={(v) => { if (!v) loopToDelete = null; }} title="Delete loop" class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden">
        <div>
          <div class="flex flex-col px-[15px] pt-[13px] pb-[11px] gap-1">
            <div class="flex items-center">
              <span class="text-[13px] font-semibold text-t1">Delete loop <span class="font-bold">{ltd.task_key || ltd.goal.slice(0, 20)}</span>?</span>
              <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
            </div>
            <p class="text-[11px] text-t3">This loop has {loopSessionCount} session{loopSessionCount !== 1 ? 's' : ''}.</p>
          </div>
          <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors" onclick={() => { loopToDelete = null; focusTerminal(); }}>
              <span class="flex-1 text-[13.5px] text-t1">Cancel</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">n</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors text-t1" onclick={() => { const id = ltd.id; loopToDelete = null; focusTerminal(); deleteLoopOnly(id); }}>
              <span class="flex-1 text-[13.5px]">Delete loop only</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">l</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-red-500/10 transition-colors text-red-400" onclick={() => { const id = ltd.id; loopToDelete = null; focusTerminal(); deleteLoopAndSessions(id); }}>
              <span class="flex-1 text-[13.5px]">Delete loop and sessions</span>
              <span class="font-mono text-[10px] text-red-400/70 border border-red-500/30 rounded-[5px] px-1.5 py-[2px]">a</span>
            </button>
          </div>
        </div>
      </SharedDialog>
    {/if}
    {#if showQuitConfirm}
      <SharedDialog open={true} onOpenChange={(v) => { if (!v) showQuitConfirm = false; }} title="Quit" class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden">
        <div>
          <div class="flex flex-col px-[15px] pt-[13px] pb-[11px] gap-1">
            <div class="flex items-center">
              <span class="text-[13px] font-semibold text-t1">{quitDirectCount} active session{quitDirectCount > 1 ? 's' : ''} will be terminated.</span>
              <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
            </div>
            <p class="text-[11px] text-t3">Direct sessions don't survive app quit.</p>
          </div>
          <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors" onclick={() => { showQuitConfirm = false; }}>
              <span class="flex-1 text-[13.5px] text-t1">Cancel</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">n</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-red-500/10 transition-colors text-red-400" onclick={() => { showQuitConfirm = false; getCurrentWindow().destroy(); }}>
              <span class="flex-1 text-[13.5px]">Quit</span>
              <span class="font-mono text-[10px] text-red-400/70 border border-red-500/30 rounded-[5px] px-1.5 py-[2px]">q</span>
            </button>
          </div>
        </div>
      </SharedDialog>
    {/if}

    {#if showNewItemModal}
      <SharedDialog open={true} onOpenChange={(v) => { if (!v) showNewItemModal = false; }} title="New…" preventOpenAutoFocus={true} class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden">
        <div>
          <div class="flex items-center px-[15px] pt-[13px] pb-[11px]">
            <span class="text-[13px] font-semibold text-t1">New…</span>
            <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
          </div>
          <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] bg-accent-bg" onclick={() => { showNewItemModal = false; taskPrefill = null; showSessionForm = true; }}>
              <span class="w-[22px] h-[22px] rounded-[7px] flex items-center justify-center font-mono text-[11px] bg-panel-hi text-t2">›_</span>
              <span class="flex-1 text-[13.5px] text-t1">Session</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">s</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors" onclick={() => { showNewItemModal = false; showTaskForm = true; }}>
              <span class="w-[22px] h-[22px] rounded-[7px] flex items-center justify-center font-mono text-[11px] bg-panel-hi text-t2">☰</span>
              <span class="flex-1 text-[13.5px] text-t1">Task</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel-hi">t</span>
            </button>
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] hover:bg-panel-hi transition-colors" onclick={() => { showNewItemModal = false; showLoopForm = true; }}>
              <span class="w-[22px] h-[22px] rounded-[7px] flex items-center justify-center font-mono text-[11px] bg-panel-hi text-t2">⟳</span>
              <span class="flex-1 text-[13.5px] text-t1">Loop (experimental)</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel-hi">l</span>
            </button>
          </div>
        </div>
      </SharedDialog>
    {/if}

    {#if showLogViewer}
      <div class="absolute inset-0 z-30">
        <LogViewer onClose={() => { showLogViewer = false; }} />
      </div>
    {/if}
    </div>
    <!-- Keyboard helper bar -->
    <KeyboardHelperBar />
  </section>

  {#if fileExplorerVisible && activeSessionId}
    {@const activeS = sessions.find(s => s.id === activeSessionId)}
    {@const activeProject = projects.find(p => p.id === activeS?.project_id)}
    {@const explorerRoot = activeS?.worktree_path ?? activeProject?.path ?? ""}
    {#if explorerRoot}
      <FileExplorer
        rootPath={explorerRoot}
        sessionId={activeSessionId}
        visible={true}
        activeFilePath={editorFileName[activeSessionId] ?? null}
        modifiedPaths={editorModified[activeSessionId] ? new Set([editorFileName[activeSessionId] ?? ""].filter(Boolean)) : new Set()}
        onOpenFile={(path) => { if (activeSessionId) openFileInTree(activeSessionId, path); }}
        onPinFile={(path) => { if (activeSessionId) openFileInTree(activeSessionId, path); }}
        onFocus={() => focusExplorer()}
      />
    {/if}
  {/if}
  </div>
</main>

{#if modalPlugin && modalContribution && activePluginSessionContext}
  <FormDialog
    title={modalContribution.label}
    class="min-h-[min(360px,85vh)]"
    preventEscapeClose={false}
    preventOpenAutoFocus={true}
    onClose={closePluginContributionModal}
  >
    <div>
      <PluginContributionHost
        plugin={modalPlugin}
        contribution={modalContribution}
        session={activePluginSessionContext}
        getFocusedAgentSession={() => activePluginSessionContext}
        onNavigate={(pluginId, contributionId) => {
          closePluginContributionModal();
          openPluginContribution(pluginId, contributionId);
        }}
        onClose={closePluginContributionModal}
        onOpenPreferences={openPreferences}
        closeOnEscape={true}
        autofocus={true}
      />
    </div>
  </FormDialog>
{/if}

{#if getSnackbarMessage()}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed bottom-4 left-4 z-[100] max-w-lg cursor-pointer rounded-lg {getSnackbarType() === 'error' ? 'bg-red-600' : getSnackbarType() === 'warning' ? 'bg-amber-600' : getSnackbarType() === 'info' ? 'bg-blue-600' : 'bg-green-600'} px-4 py-3 shadow-lg" onclick={() => { navigator.clipboard.writeText(getSnackbarMessage()!); dismissSnackbar(); }} title="Click to copy and dismiss">
    <p class="text-sm text-white font-mono break-all">{getSnackbarMessage()}</p>
    <p class="text-xs {getSnackbarType() === 'error' ? 'text-red-200' : getSnackbarType() === 'warning' ? 'text-amber-100' : getSnackbarType() === 'info' ? 'text-blue-100' : 'text-green-200'} mt-1">Click to dismiss</p>
  </div>
{/if}

{#each interactionPluginContributions as { plugin, contribution } (`${plugin.id}:${contribution.id}`)}
  <div class="pointer-events-none fixed inset-0 z-[90]" data-plugin-interaction-host={`${plugin.id}:${contribution.id}`}>
    <PluginContributionHost {plugin} {contribution} onNavigate={openPluginContribution} onClose={leavePluginWorkspace} onOpenPreferences={openPreferences} />
  </div>
{/each}
<PostMergePrompt />
<UpdateToast />
