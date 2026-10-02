<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import { sessions as sessionsApi, pty, notify, sessionLogs, editor as editorApi, updater } from "./lib/api";
  import { sessionTaskProjectId, type Session, type Project, type TaskItem } from "./lib/types";
  import { focusEditor, focusTerminal, refocusTerminal, focusExplorer, focusSidebar, getActiveZone, toggleExplorerFocus } from "./lib/focus.svelte";
  import { isTerminalPaneFocused, releaseTerminalDomFocus } from "./lib/terminal-focus";
  import * as projectStore from "./lib/project-store.svelte";
  import * as taskStore from "./lib/task-store.svelte";
  import { installKeyboardRouter, matchChord, MOD_LABEL, isPlatformMod, MOD_ENTER_HINT, IS_WINDOWS } from "./lib/keyboard";
  import { findPluginShortcut } from "./lib/plugin-shortcuts";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import * as navCycle from "./lib/session-nav-cycle.svelte";
  import { sidebarNavigationOrder } from "./lib/sidebar-model";
  import { getSidebarModel } from "./lib/sidebar-model-store.svelte";
  import { isLoopId, parseLoopId, isTaskWorkspaceId, parseTaskWorkspaceId, toTaskWorkspaceId } from "./lib/sidebar-session-order";
  import { isTerminal, isActive as isLoopActive } from "./lib/loop-status";
  import { loadSettings, getSettings, isDark } from "./lib/settings.svelte";
  import { pluginPreferencesLocation, settingsLocationQuery, type SettingsLocation } from "./lib/settings-registry";
  import { shouldShowOnboarding } from "./lib/onboarding";
  import Onboarding from "./components/Onboarding.svelte";
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
  import ProviderSessionView from "./components/ProviderSessionView.svelte";
  import { PROVIDER_BACKEND, runtimeProviders } from "./lib/plugin-providers";
  import { forgetHandoff, handoffTabFor, startHandoff } from "./lib/provider-handoff";
  import type { PluginInventory, PluginSessionAction, PluginSessionAdvisory, PluginSessionCompletion, PluginUiContribution } from "./lib/types";
  import * as loopStore from "./lib/loop-store.svelte";
  import { loops as loopsApi, plugins as pluginsApi, providerSessions } from "./lib/api";
  import { focusMergePrompt, getPrompt, showMergePrompt } from "./lib/post-merge-prompt.svelte";
  import { taskWorkspaceLayout as workspaceLayout, toPaneTabs, workspaceOf, type PaneTab, type WorkspaceAgent, type WorkspaceIdentity } from "./lib/task-workspace-layout.svelte";
  import { activeTabOf, findLeaf, tabsOf, type LeafNode, type NavDirection, type SplitDirection, type TabEntry } from "./lib/layout-tree";
  import { pressTab as pressTabForDrag, tabDrag, type TabDropTarget } from "./lib/tab-drag.svelte";
  import { dropPositionToViewport, droppedPathsText } from "./lib/dropped-paths";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { ptyKeySessionId } from "./lib/pty-key";
  import { saveActiveEditorResource } from "./lib/editor-resources";
  import { getMruList } from "./lib/mru.svelte";
  import * as orchestrator from "./lib/session-orchestrator.svelte";
  import UpdateToast from "./components/UpdateToast.svelte";
  import { initUpdateListener, focusUpdateToast, getUpdateState, setLaunchUpdateAvailable } from "./lib/updater.svelte";
  import SplitContainer from "./components/SplitContainer.svelte";
  import TabStrip from "./components/TabStrip.svelte";

  // ─── UI-only state ──────────────────────────────────────────────────────────
  let showProjectForm = $state(false);
  let showSessionForm = $state(false);
  let showTaskForm = $state(false);
  let sidebarVisible = $state(true);
  let commandMenuOpen = $state(false);
  let commandMenuFileMode = $state(false);
  let commandMenuRenameId = $state<string | null>(null);
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
  const symphonyStatus = $derived(orchestrator.getSymphonyStatus());
  const zone = $derived(getActiveZone());
  const activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);
  const activeTaskWorkspace = $derived.by(() => {
    if (selectedTaskWorkspace) return selectedTaskWorkspace;
    if (!activeSession?.task_key) return null;
    const project = projects.find((candidate) => candidate.id === sessionTaskProjectId(activeSession));
    const task = project ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === activeSession.task_key) : undefined;
    return task && project ? { task, project } : null;
  });
  const activeTaskSessions = $derived(
    activeTaskWorkspace
      ? sessions.filter((session) => workspaceOf(session).key === toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key))
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
  const sidebarSessionOrder = $derived(sidebarNavigationOrder(getSidebarModel()));

  // ─── TaskWorkspace layout ───────────────────────────────────────────────────
  const layoutTree = $derived(workspaceLayout.layout?.tree ?? null);
  const focusedLeafId = $derived(workspaceLayout.layout?.focusedLeafId ?? null);
  const hasMultiplePanes = $derived(workspaceLayout.isSplit);
  // Split shortcuts act on the visible layout only: never behind a loop
  // dashboard, a plugin page or an empty TaskWorkspace.
  const canSplit = $derived(
    !!activeSessionId && !!layoutTree && !isEmptyTaskWorkspace && !activeLoopId && !activePluginId,
  );
  // A single pane shows its tabs in the titlebar; split panes each get their own tab bar.
  const singlePane = $derived(layoutTree?.type === "leaf" ? layoutTree : null);
  const titlebarTabs = $derived(singlePane ? paneTabs(singlePane) : []);

  // Editor tabs with unsaved changes, by pty key.
  let modifiedEditorTabs = $state<ReadonlySet<string>>(new Set());
  const explorerActiveFile = $derived.by(() => {
    const tab = workspaceLayout.focusedTab();
    return tab?.type === "editor" ? (tab.filePath ?? null) : null;
  });
  const explorerModifiedPaths = $derived(new Set(
    tabsOf(workspaceLayout.layout)
      .filter((tab) => tab.type === "editor" && tab.filePath && modifiedEditorTabs.has(tab.ptyKey) && ptyKeySessionId(tab.ptyKey) === activeSessionId)
      .map((tab) => tab.filePath!),
  ));

  function setEditorTabModified(ptyKey: string, modified: boolean): void {
    if (modifiedEditorTabs.has(ptyKey) === modified) return;
    const next = new Set(modifiedEditorTabs);
    if (modified) next.add(ptyKey);
    else next.delete(ptyKey);
    modifiedEditorTabs = next;
  }

  const showOnboarding = $derived(shouldShowOnboarding(getSettings()));

  // ─── Terminal DOM focus ─────────────────────────────────────────────────────

  // Every dialog that owns the keyboard, not only those that gated `focused`
  // before: a trailing focus request would otherwise hand xterm DOM focus behind
  // one, and xterm swallows the keys the dialog needs. Dialogs App does not model
  // are caught by the DOM probe inside releaseTerminalDomFocus.
  const keyboardModalOpen = $derived(
    showNewItemModal || !!sessionToDelete || showTaskForm || showProjectForm || !!modalPluginId
      || showSessionForm || showLoopForm || commandMenuOpen || showShortcuts
      || !!projectToDelete || !!loopToDelete || showQuitConfirm || showOnboarding,
  );
  const terminalKeyboardOwnership = $derived({
    zone,
    modalOpen: keyboardModalOpen,
    pluginOverlayActive: !!activePluginId,
  });

  // See releaseTerminalDomFocus: a stranded terminal silently swallows sidebar
  // navigation, so release when ownership is lost...
  $effect(() => {
    releaseTerminalDomFocus(terminalKeyboardOwnership);
  });

  // ...and on focus arrival. Bubble phase, so a genuine click has already run
  // Terminal's onFocused -> focusTerminal() and ownership reads true by now.
  $effect(() => {
    const onFocusIn = (): void => releaseTerminalDomFocus(terminalKeyboardOwnership);
    window.addEventListener("focusin", onFocusIn);
    return () => window.removeEventListener("focusin", onFocusIn);
  });

  // Layouts load once sessions are known, so a restored layout can be reconciled against them.
  let sessionsLoaded = $state(false);

  $effect(() => {
    if (!sessionsLoaded && sessions.length > 0) {
      sessionsLoaded = true;
    }
  });

  function workspaceForSession(sessionId: string): WorkspaceIdentity | null {
    const session = sessions.find((candidate) => candidate.id === sessionId);
    return session ? workspaceOf(session) : null;
  }

  function workspaceSessions(workspace: WorkspaceIdentity): Session[] {
    return sessions.filter((session) => workspaceOf(session).key === workspace.key);
  }

  function workspaceAgents(workspace: WorkspaceIdentity): WorkspaceAgent[] {
    return workspaceSessions(workspace).map((session) => ({
      sessionId: session.id,
      label: session.name || session.branch || "Agent",
      icon: session.provider ? "bot" : "terminal",
    }));
  }

  // The selected session decides which workspace is shown. Selecting another
  // agent of the same task keeps the shared layout and only brings its tab forward.
  // Keyed by identity rather than the session list: an empty TaskWorkspace clears
  // the layout while the selection stays put, and an unrelated session update must
  // not load the hidden workspace back in behind it.
  const activeWorkspaceKey = $derived(
    activeSessionId && sessionsLoaded ? (workspaceForSession(activeSessionId)?.key ?? null) : null,
  );
  $effect(() => {
    if (!activeWorkspaceKey || !activeSessionId) return;
    const sessionId = activeSessionId;
    untrack(() => {
      const workspace = workspaceForSession(sessionId);
      if (workspace) void showWorkspace(workspace, sessionId, orchestrator.isSelectionExplicit(sessionId));
    });
  });

  // Keep the loaded workspace's tabs in step with its sessions (added, renamed, archived).
  $effect(() => {
    const workspace = workspaceLayout.workspace;
    if (!workspace || workspaceLayout.loading) return;
    const agents = workspaceAgents(workspace);
    untrack(() => workspaceLayout.reconcile(agents));
  });

  /** Show a workspace, then apply the session selection its restored layout implies. */
  async function showWorkspace(workspace: WorkspaceIdentity, sessionId: string, selectionIsExplicit: boolean): Promise<void> {
    const shown = await workspaceLayout.show(workspace, {
      agents: workspaceAgents(workspace),
      selectedSessionId: sessionId,
      selectionIsExplicit,
    });
    if (!shown) return;
    if (shown.adoptedSessionId) orchestrator.selectSession(shown.adoptedSessionId);
    if (shown.restored) requestFocusedTerminalFocus();
  }

  /** Press on a tab: dragging it drops into a tab bar, into a pane, or splits a pane. */
  function pressTab(e: PointerEvent, ptyKey: string): void {
    pressTabForDrag(ptyKey, e, dropTab);
  }

  function dropTab(ptyKey: string, target: TabDropTarget): void {
    if (target.kind === "strip") workspaceLayout.moveTab(ptyKey, target.paneId, target.index);
    else if (target.zone !== "center") workspaceLayout.splitWithTab(ptyKey, target.paneId, target.zone);
    else if (workspaceLayout.findTab(ptyKey)?.leaf.id !== target.paneId) workspaceLayout.moveTab(ptyKey, target.paneId);
    else return;
    syncFocusedTabToSelection();
  }

  /** Pane a file dragged from the OS is over, highlighted as the drop target. */
  let fileDropPane = $state<string | null>(null);

  /** The pane under a native drag-drop position. */
  function paneAt(position: { x: number; y: number }): string | null {
    const point = dropPositionToViewport(position, { windows: IS_WINDOWS, pixelRatio: devicePixelRatio });
    const element = document.elementFromPoint(point.x, point.y);
    return element?.closest<HTMLElement>("[data-pane-drop]")?.dataset.paneDrop ?? null;
  }

  /** Type dropped file paths into the terminal in front of a pane, like a terminal app. */
  async function typeDroppedPaths(paneId: string, paths: string[]): Promise<void> {
    const leaf = workspaceLayout.layout ? findLeaf(workspaceLayout.layout, paneId) : null;
    const tab = activeTabOf(leaf);
    if (!tab || !isTerminalTab(tab)) return;
    const { text, skipped } = droppedPathsText(paths, { windows: IS_WINDOWS });
    if (skipped.length > 0) {
      showSnackbar(
        skipped.length === 1
          ? "Skipped a dropped file whose name cannot be typed safely"
          : `Skipped ${skipped.length} dropped files whose names cannot be typed safely`,
        "error",
      );
    }
    if (!text) return;
    workspaceLayout.focusTab(tab.ptyKey);
    selectTerminalTab(tab.ptyKey);
    try {
      if (!(await pty.write(tab.ptyKey, Array.from(new TextEncoder().encode(text))))) {
        throw new Error("the terminal is not attached");
      }
      orchestrator.recordUserInput(ptyKeySessionId(tab.ptyKey));
    } catch (error) {
      showSnackbar(`Failed to type dropped path: ${error instanceof Error ? error.message : String(error)}`, "error");
    }
  }

  function paneTabs(leaf: LeafNode): PaneTab[] {
    return toPaneTabs(leaf, (tab) => tab.type === "agent" ? crossProjectName(ptyKeySessionId(tab.ptyKey)) : undefined);
  }

  /** Names the repo an agent runs in when it differs from its task's project. */
  function crossProjectName(sessionId: string): string | undefined {
    const session = sessions.find((candidate) => candidate.id === sessionId);
    if (!session || sessionTaskProjectId(session) === session.project_id) return undefined;
    return projects.find((project) => project.id === session.project_id)?.name;
  }

  /** Bring a tab forward from a tab strip; a terminal tab also selects its session. */
  function selectPaneTab(ptyKey: string): void {
    const tab = workspaceLayout.focusTab(ptyKey);
    if (tab && isTerminalTab(tab)) selectTerminalTab(ptyKey);
  }

  function isTerminalTab(tab: TabEntry): boolean {
    return tab.type === "agent" || tab.type === "shell";
  }

  const SPLIT_DIRECTIONS: Record<string, NavDirection> = {
    focus_split_left: "left", focus_split_right: "right", focus_split_up: "up", focus_split_down: "down",
    move_tab_left: "left", move_tab_right: "right", move_tab_up: "up", move_tab_down: "down",
  };

  function handleSplitAction(actionType: string): void {
    if (!canSplit) return;
    if (actionType === "split_vertical") return splitPane("vertical");
    if (actionType === "split_horizontal") return splitPane("horizontal");
    if (actionType === "close_split") workspaceLayout.closePane();
    else if (actionType.startsWith("focus_split_")) workspaceLayout.focusDirection(SPLIT_DIRECTIONS[actionType]);
    else if (actionType.startsWith("move_tab_")) workspaceLayout.moveFocusedTab(SPLIT_DIRECTIONS[actionType]);
    syncFocusedTabToSelection();
  }

  /** Split the focused pane and open a login shell for the focused agent in the new pane. */
  function splitPane(direction: SplitDirection): void {
    if (!activeSessionId) return;
    if (!workspaceLayout.openShell(activeSessionId, { split: direction })) return;
    // Wait for Terminal to mount + open before refocusing
    tick().then(() => requestAnimationFrame(() => refocusTerminal()));
  }

  /** Open a shell tab for the focused agent in a pane (the focused one by default). */
  function openShellTab(paneId?: string): void {
    if (!activeSessionId) return;
    if (!workspaceLayout.openShell(activeSessionId, { paneId })) return;
    refocusTerminal();
  }

  /**
   * Close a tab. Agent tabs represent durable sessions, so closing one parks the
   * agent rather than destroying its worktree; a task workspace may end up empty.
   */
  async function closeTab(ptyKey: string): Promise<void> {
    let outcome;
    try {
      outcome = await workspaceLayout.closeTab(ptyKey);
    } catch (error) {
      showSnackbar(`Failed to close shell tab: ${error instanceof Error ? error.message : String(error)}`, "error");
      return;
    }
    if (outcome === "starting") {
      showSnackbar("Terminal editor is still starting", "error");
    } else if (outcome === "agent") {
      const session = sessions.find((candidate) => candidate.id === ptyKeySessionId(ptyKey));
      if (!session) return;
      try {
        await orchestrator.parkSession(session);
      } catch (error) {
        showSnackbar(`Failed to close session: ${error instanceof Error ? error.message : String(error)}`, "error");
      }
    } else if (outcome === "closed") {
      await tick();
      refocusTerminal();
    }
  }

  function closeFocusedTab(): void {
    const tab = workspaceLayout.focusedTab();
    if (tab) void closeTab(tab.ptyKey);
  }

  function preserveKeyboardSelectedTerminal(entry: TabEntry): void {
    if (!isTerminalTab(entry)) return;
    if (ptyKeySessionId(entry.ptyKey) !== activeSessionId) selectTerminalTab(entry.ptyKey);
    // Session synchronization can focus the agent tab. Restore the specific
    // keyboard-selected PTY after that reactive update settles.
    tick().then(() => requestAnimationFrame(() => {
      workspaceLayout.focusTab(entry.ptyKey);
      requestTerminalFocus(entry.ptyKey);
    }));
  }

  /** Move to the next (1) or previous (-1) tab of the focused pane. */
  function cycleTab(delta: number): void {
    const tab = workspaceLayout.cycleTab(delta);
    if (tab) preserveKeyboardSelectedTerminal(tab);
  }

  function toggleDiff(): void {
    if (!activeSessionId) return;
    if (workspaceLayout.toggleDiff(activeSessionId) === "closed") {
      tick().then(() => refocusTerminal());
    }
  }

  async function openTerminalEditor(sessionId: string, filePath: string): Promise<void> {
    try {
      if (await workspaceLayout.openTerminalEditor(sessionId, filePath)) {
        tick().then(() => requestAnimationFrame(() => refocusTerminal()));
      }
    } catch (error) {
      showSnackbar(`Failed to open terminal editor: ${error}`, "error");
    }
  }

  async function handleShellAttachError(ptyKey: string, error: unknown): Promise<void> {
    // A shell that never started would otherwise linger as a dead pane; this
    // close is not the user's, so it must not steal focus.
    const message = workspaceLayout.isStarting(ptyKey) ? "Failed to open terminal editor" : "Failed to start shell";
    showSnackbar(`${message}: ${error}`, "error");
    try {
      await workspaceLayout.shellFailedToStart(ptyKey);
    } catch (closeError) {
      console.warn("Failed to clean up shell tab", ptyKey, closeError);
    }
  }

  /** Open a file with the globally configured editor. */
  async function openFile(sessionId: string, filePath: string): Promise<void> {
    if (filePath.split(/[/\\]/).includes("..")) return;
    const result = await openFileWithConfiguredEditor(getSettings().editor, {
      openEmbedded: () => { workspaceLayout.openEditor(sessionId, filePath); },
      openTerminal: () => openTerminalEditor(sessionId, filePath),
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
  function syncFocusedTabToSelection(): void {
    const tab = workspaceLayout.focusedTab();
    if (!tab || !isTerminalTab(tab)) return;
    if (ptyKeySessionId(tab.ptyKey) !== activeSessionId) selectTerminalTab(tab.ptyKey);
  }

  function closeOverlays(): void {
    showSessionForm = false; showProjectForm = false; projectToEdit = null; showShortcuts = false; showNewItemModal = false; showTaskForm = false; closePluginContributionModal(); showLoopForm = false; sessionToDelete = null; commandMenuOpen = false; commandMenuFileMode = false; commandMenuRenameId = null;
  }

  // Setup can restart mid-session; anything left open would sit under the wizard and keep the keyboard.
  $effect(() => {
    if (!showOnboarding) return;
    untrack(() => {
      closeOverlays();
      projectToDelete = null;
      loopToDelete = null;
    });
  });

  // ─── Provider session terminal handoff ─────────────────────────────────────

  async function handoffProviderSession(sessionId: string): Promise<void> {
    await startHandoff(sessionId, (command, label) => workspaceLayout.openCommand(sessionId, command, label));
  }

  async function handbackProviderSession(sessionId: string): Promise<void> {
    const ptyKey = handoffTabFor(sessionId);
    const closed = ptyKey ? await workspaceLayout.closeTab(ptyKey) : "missing";
    if (closed === "starting") throw new Error("The terminal is still starting.");
    // Closing the tab hands the session back; without one there is nothing left to close.
    if (closed === "missing") {
      forgetHandoff(sessionId);
      await providerSessions.handback(sessionId);
    }
  }

  // ─── Project management ─────────────────────────────────────────────────────
  async function openPreferences(location?: SettingsLocation) {
    const existing = await WebviewWindow.getByLabel("preferences");
    if (existing) {
      if (location) await emitTo("preferences", "preferences-navigate", location).catch((error) => console.warn("Failed to navigate Preferences:", error));
      existing.setFocus();
      return;
    }
    const [search, hash] = location ? settingsLocationQuery(location) : ["?page=preferences", ""];
    new WebviewWindow("preferences", { url: `index.html${search}${hash}`, title: "Preferences", width: 920, height: 680, minWidth: 760, minHeight: 520, parent: getCurrentWindow(), resizable: true, minimizable: false, maximizable: false });
  }

  async function doRename(id: string, name: string) {
    renamingSessionId = null;
    try {
      await sessionsApi.rename(id, name);
      orchestrator.updateSessionName(id, name);
    } catch (error) {
      showSnackbar(`Failed to rename session: ${error}`);
    }
    focusTerminal();
  }

  function openSessionRename(sessionId: string): void {
    commandMenuFileMode = false;
    commandMenuRenameId = sessionId;
    commandMenuOpen = true;
  }

  /** Double-clicking an agent tab renames its session. */
  function renameFromTab(ptyKey: string): void {
    if (workspaceLayout.findTab(ptyKey)?.tab.type === "agent") openSessionRename(ptyKeySessionId(ptyKey));
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
            const project = projects.find((candidate) => candidate.id === sessionTaskProjectId(found));
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

  /**
   * `focusPtyKey` is the terminal that takes keyboard focus, when it is not the
   * agent's own tab (a shell of that session, clicked or selected).
   */
  function selectWorkspaceSession(sessionId: string, opts: { explicit?: boolean; focusPtyKey?: string } = {}): void {
    const session = sessions.find((candidate) => candidate.id === sessionId);
    if (session?.task_key) {
      const project = projects.find((candidate) => candidate.id === sessionTaskProjectId(session));
      const task = project ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === session.task_key) : undefined;
      if (project && task) {
        selectedTaskWorkspace = { project, task };
        touchWorkspaceMru(toTaskWorkspaceId(project.id, task.key));
      } else {
        // The task could not be resolved: done tasks are filtered out of the
        // listing, and another project's tasks may not be loaded yet. Keeping the
        // previous selection would pin the main pane to the wrong task, so fall
        // back to deriving the workspace from the newly selected session.
        selectedTaskWorkspace = null;
      }
    } else {
      selectedTaskWorkspace = null;
    }
    leavePluginWorkspace();
    loopStore.setActiveLoopId(null);
    // `selectWorkspaceTask` passes explicit: false — its first-linked session is an
    // arbitrary entry point and must not override a remembered tab.
    orchestrator.selectSession(sessionId, { explicit: opts.explicit ?? true });
    requestTerminalFocus(opts.focusPtyKey ?? sessionId);
  }

  /** Select the session of a terminal tab, keeping keyboard focus on that tab. */
  function selectTerminalTab(ptyKey: string): void {
    selectWorkspaceSession(ptyKeySessionId(ptyKey), { focusPtyKey: ptyKey });
  }

  function openSessionForTask(task: TaskItem, project: Project): void {
    taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: task.title, prompt: "", baseBranch: task.base_branch, projectId: project.id };
    showSessionForm = true;
  }

  /** Generic "new session" entry points start from the focused task, if any. */
  function openNewSessionForm(): void {
    if (activeTaskWorkspace && !activeLoopId && !activePluginId) { openSessionForTask(activeTaskWorkspace.task, activeTaskWorkspace.project); return; }
    taskPrefill = null;
    showSessionForm = true;
  }

  function selectWorkspaceTask(task: TaskItem, repoPath: string): void {
    const project = projects.find((candidate) => candidate.path === repoPath);
    if (!project) return;
    selectedTaskWorkspace = { task, project };
    touchWorkspaceMru(toTaskWorkspaceId(project.id, task.key));
    loopStore.setActiveLoopId(null);
    const linked = sessions.find((session) => sessionTaskProjectId(session) === project.id && session.task_key === task.key);
    if (linked) {
      const alreadyActive = linked.id === activeSessionId;
      selectWorkspaceSession(linked.id, { explicit: false });
      // The active session ID does not change when returning from an empty
      // TaskWorkspace. Reload its layout because the empty workspace reset the tree.
      if (alreadyActive) {
        const workspace = workspaceForSession(linked.id);
        if (workspace) void showWorkspace(workspace, linked.id, false);
      }
      // Keep the routed task authoritative even if session/task-store reconciliation lags.
      selectedTaskWorkspace = { task, project };
    } else workspaceLayout.clear();
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
        archivedTaskSessions = items.filter((session) => sessionTaskProjectId(session) === workspace.project.id && session.task_key === workspace.task.key);
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
      return session?.task_key ? toTaskWorkspaceId(sessionTaskProjectId(session), session.task_key) : id;
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
    const tab = workspaceLayout.focusedTab();
    if (tab && isTerminalTab(tab)) requestTerminalFocus(tab.ptyKey);
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
    const cleanupTaskListener = taskStore.startTaskEventListener(() => projectStore.getProjects().map((p) => p.path));
    const unlistenSettings = listen("settings-changed", () => { loadSettings().then(() => loadTheme()); });
    const unlistenCleanup = listen<string>("cleanup-error", (event) => { showSnackbar(event.payload); });
    const unlistenShellPtyExit = listen<{ pty_key: string }>("pty-exited", (event) => {
      void workspaceLayout.shellExited(event.payload.pty_key).catch((error) => {
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
      if (event.defaultPrevented || showOnboarding) return;
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
    const unlistenFileDrop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "leave") fileDropPane = null;
      else if (payload.type === "drop") {
        const paneId = paneAt(payload.position);
        fileDropPane = null;
        if (paneId) void typeDroppedPaths(paneId, payload.paths);
      } else fileDropPane = paneAt(payload.position);
    });
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
          closeOverlays();
        } else if (action.type === "command_palette") { commandMenuOpen = !commandMenuOpen; commandMenuRenameId = null; }
        else if (action.type === "open_preferences") { openPreferences(); }
        else if (action.type === "show_shortcuts") { showShortcuts = !showShortcuts; }
        else if (action.type === "new_tab") { openShellTab(); }
        else if (action.type === "close_tab") { closeFocusedTab(); }
        else if (action.type === "next_tab") { cycleTab(1); }
        else if (action.type === "prev_tab") { cycleTab(-1); }
        else if (action.type === "next_session") {
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeTaskWorkspace ? toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key) : activeSessionId ?? undefined;
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, currentId, 1);
          else navCycle.advance(1);
        } else if (action.type === "prev_session") {
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeTaskWorkspace ? toTaskWorkspaceId(activeTaskWorkspace.project.id, activeTaskWorkspace.task.key) : activeSessionId ?? undefined;
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, currentId, -1);
          else navCycle.advance(-1);
        }
        else if (action.type === "toggle_diff") { toggleDiff(); }
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
      () => getActiveZone() === "editor" && workspaceLayout.focusedTab()?.type === "editor",
      () => !!document.activeElement?.closest('[data-form-keyboard]'),
      () => showOnboarding,
    );

    function onModalKeydown(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (showNewItemModal) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape') { showNewItemModal = false; }
        else if (e.key === 's') { showNewItemModal = false; openNewSessionForm(); }
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

    return () => { pluginListenersDisposed = true; window.removeEventListener("keydown", onPluginShortcut, true); unlistenFileDrop.then((fn) => fn()); cleanup(); cleanupEvents(); cleanupSymphony(); cleanupLoopListener(); cleanupTaskListener(); unlistenSettings.then((fn) => fn()); unlistenCleanup.then((fn) => fn()); unlistenShellPtyExit.then((fn) => fn()); unlistenPluginRuntime.then((fn) => fn()); unlistenPluginActions.then((fn) => fn()); unlistenPluginAdvisory.then((fn) => fn()); unlistenPluginCompletion.then((fn) => fn()); unlistenClose.then((fn) => fn()); window.removeEventListener("keydown", onModalKeydown, true); window.removeEventListener("keyup", onKeyUp); window.removeEventListener("blur", onBlur); };
  });
</script>

<main class="flex flex-col h-screen" inert={showOnboarding}>
  <Titlebar
    projectName={activeProjectName}
    sessionName={activeSessionName}
    {sidebarVisible}
    tabs={isEmptyTaskWorkspace || hasMultiplePanes ? [] : titlebarTabs}
    activeTabId={activeTabOf(singlePane)?.ptyKey ?? ""}
    runningCount={sessions.filter(s => s.status === 'active').length}
    activeProvider={activeSession?.provider ?? null}
    onSelectTab={(ptyKey) => {
      leavePluginWorkspace();
      loopStore.setActiveLoopId(null);
      selectPaneTab(ptyKey);
    }}
    onAddTab={() => openShellTab(singlePane?.id)}
    onTabDoubleClick={renameFromTab}
    paneId={singlePane?.id}
    onTabPress={pressTab}
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
        onCreateSession={openNewSessionForm}
        onSessionsChanged={() => { orchestrator.loadSessions(); taskStore.refresh(projects.map((p) => p.path)); }}
        onSelectLoop={selectWorkspaceLoop}
        onStartLoop={(id) => { loopsApi.start(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onTickLoop={(id) => { loopsApi.tick(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onStopLoop={(id) => { loopsApi.stop(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onDeleteLoop={(id) => { const loop = projects.flatMap(p => loopStore.getLoopsForProject(p.id)).find(l => l.id === id); if (!loop) return; const hasSessions = (loopStore.getSessionsForLoop(id) ?? []).length > 0; if (hasSessions) { loopToDelete = loop; } else { deleteLoopOnly(id); } }}
        onDeleteLoopSession={(session, loopId) => { const loop = projects.flatMap(p => loopStore.getLoopsForProject(p.id)).find(l => l.id === loopId); if (loop && isLoopActive(loop.status)) { showSnackbar("Stop the loop before deleting its sessions"); } else { orchestrator.deleteSession(session); } }}
        selectedTaskWorkspace={activeTaskWorkspace ? { projectId: activeTaskWorkspace.project.id, taskKey: activeTaskWorkspace.task.key } : null}
        selectedLoopId={activeLoopId}
        onToggleDiff={toggleDiff}
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
        runtimeProviders={runtimeProviders(pluginInventory)}
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
      renameSessionId={commandMenuRenameId}
      onOpenChange={(v) => { commandMenuOpen = v; if (!v) { commandMenuFileMode = false; commandMenuRenameId = null; } }}
      onSelectSession={(id) => { selectWorkspaceSession(id); focusTerminal(); }}
      onArchiveSession={() => { if (activeSessionId) { const s = sessions.find(x => x.id === activeSessionId); if (s) orchestrator.archiveSession(s); } }}
      onDeleteSession={() => { if (activeSessionId) { const s = sessions.find(x => x.id === activeSessionId); if (s) sessionToDelete = s; } }}
      onRenameSession={doRename}
      onRestoreSession={async (id) => { await sessionsApi.restore(id); await orchestrator.loadSessions(); }}
      onDestroyArchivedSession={async (id) => { await sessionsApi.destroy(id); }}
      onNewSession={() => { if (projects.length === 0) openAddProject(); else openNewSessionForm(); }}
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
      onSelectTask={(task, repoPath) => { selectWorkspaceTask(task, repoPath); focusTerminal(); }}
      onCreateTask={() => { showTaskForm = true; }}
      onToggleDiff={toggleDiff}
      onOpenFile={(path) => { if (activeSessionId) openFile(activeSessionId, path); }}
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
      {@const activeEntry = activeTabOf(leaf)}
      {@const isFocusedLeaf = leaf.id === focusedLeafId}
      <!-- Click-to-focus on the pane container. Focus is also reachable from the keyboard
           via the pane navigation shortcuts, so no key handler is duplicated here. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="split-leaf {hasMultiplePanes ? '' : 'split-leaf-single'}"
        class:split-leaf-focused={isFocusedLeaf && hasMultiplePanes}
        role="group"
        aria-label="Split pane"
        onclick={(event) => {
          workspaceLayout.focusPane(leaf.id);
          if (activeEntry && isTerminalTab(activeEntry)) selectTerminalTab(activeEntry.ptyKey);
          if (!(event.target instanceof Element && event.target.closest("[data-editor-tab]"))) {
            focusTerminal();
          }
        }}
      >
        {#if hasMultiplePanes}
        <div class="flex items-stretch h-[38px] bg-chrome border-b border-border shrink-0">
          <TabStrip
            tabs={paneTabs(leaf)}
            activeTabId={activeEntry?.ptyKey ?? ""}
            focused={isFocusedLeaf}
            showAddButton={true}
            showCloseButton={true}
            paneId={leaf.id}
            onSelectTab={selectPaneTab}
            onAddTab={() => openShellTab(leaf.id)}
            onClose={() => { workspaceLayout.closePane(leaf.id); syncFocusedTabToSelection(); }}
            onTabDoubleClick={renameFromTab}
            onTabPress={pressTab}
          />
        </div>
        {/if}
        <div class="split-leaf-content" data-pane-drop={leaf.id}>
          {#if tabDrag.target?.kind === "pane" && tabDrag.target.paneId === leaf.id}
            <div class="pane-drop-preview" data-zone={tabDrag.target.zone}></div>
          {:else if fileDropPane === leaf.id && activeEntry && isTerminalTab(activeEntry)}
            <div class="pane-drop-preview" data-zone="center"></div>
          {/if}
          {#each leaf.tabs as tabEntry (tabEntry.ptyKey)}
            {@const sessionId = ptyKeySessionId(tabEntry.ptyKey)}
            {@const session = sessions.find((s) => s.id === sessionId)}
            {@const isActiveInLeaf = tabEntry.ptyKey === activeEntry?.ptyKey}
            {@const project = session ? projects.find((p) => p.id === session.project_id) : null}
            {#if isTerminalTab(tabEntry)}
              {#if session && tabEntry.type === "agent" && session.backend === PROVIDER_BACKEND}
              <div class="absolute inset-0" class:hidden={!isActiveInLeaf}>
                <ProviderSessionView
                  {session}
                  inventory={pluginInventory}
                  autofocus={isActiveInLeaf && sessionId === activeSessionId}
                  onNavigate={openPluginContribution}
                  onOpenPreferences={openPreferences}
                  onHandoff={handoffProviderSession}
                  onHandback={handbackProviderSession}
                />
              </div>
              {:else if session}
              <!-- Wrapper hides inactive tabs; Terminal's visible prop also pauses during loop overlay -->
              <div class="absolute inset-0" class:hidden={!isActiveInLeaf}>
                <Terminal
                  focusRequest={terminalFocusRequest}
                  sessionId={tabEntry.ptyKey}
                  visible={isActiveInLeaf && !activeLoopId && !activePluginId}
                  focused={isTerminalPaneFocused({
                    ...terminalKeyboardOwnership,
                    isActiveTabInLeaf: isActiveInLeaf,
                    isFocusedLeaf,
                    belongsToActiveSession: sessionId === activeSessionId,
                  })}
                  exited={tabEntry.type === "agent" && session.status === "exited"}
                  skipAttach={tabEntry.type === "shell"}
                  initialCommand={tabEntry.type === "shell" ? workspaceLayout.pendingCommand(tabEntry.ptyKey) : undefined}
                  onAttached={() => {
                    if (tabEntry.type !== "shell") return;
                    workspaceLayout.shellStarted(tabEntry.ptyKey);
                    if (leaf.id === workspaceLayout.layout?.focusedLeafId) refocusTerminal();
                  }}
                  onAttachError={(error) => {
                    if (tabEntry.type === "shell") void handleShellAttachError(tabEntry.ptyKey, error);
                    else showSnackbar(String(error));
                  }}
                  onFocused={(event) => {
                    if (event.type === "focusin" && !isActiveInLeaf) return;
                    workspaceLayout.focusPane(leaf.id);
                    selectTerminalTab(tabEntry.ptyKey);
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
                    onEditFile={(filePath) => openFile(sessionId, filePath)}
                    onFileChange={(name) => workspaceLayout.setTabTitle(tabEntry.ptyKey, name)}
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
                  focused={isActiveInLeaf && !activePluginId && isFocusedLeaf && zone === "editor"}
                  onClose={() => { setEditorTabModified(tabEntry.ptyKey, false); void closeTab(tabEntry.ptyKey); }}
                  onFocusEditor={() => { workspaceLayout.focusTab(tabEntry.ptyKey); focusEditor(); }}
                  onFileChange={(name) => workspaceLayout.setTabTitle(tabEntry.ptyKey, name)}
                  onModifiedChange={(modified) => setEditorTabModified(tabEntry.ptyKey, modified)}
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

    <!-- Always render through the layout tree (a single pane is the normal view) -->
    {#if layoutTree && !isEmptyTaskWorkspace}
      <div class:hidden={!!activeLoopId || !!activePluginId} class="w-full h-full">
        <SplitContainer node={layoutTree} renderLeaf={splitLeafSnippet} />
      </div>
    {/if}

    {#if activePluginId}
      <div class="flex h-full flex-col bg-main">
        <div class="flex shrink-0 items-center gap-3 border-b border-border px-4 py-2">
          <button class="text-xs text-t2 hover:text-t1" onclick={leavePluginWorkspace}>← Back to workspace</button>
          <span class="text-sm font-medium text-t1">{activePlugin ? `${activePlugin.name} · ${activeContribution?.label ?? "Contribution"}` : "Plugin"}</span>
        </div>
        {#if activePlugin && activeContribution}
          <div class="min-h-0 flex-1"><PluginContributionHost plugin={activePlugin} contribution={activeContribution} session={activeContribution.placement === "session.panel" ? activePluginSessionContext : undefined} getFocusedAgentSession={() => activePluginSessionContext} onNavigate={openPluginContribution} onClose={leavePluginWorkspace} onOpenPreferences={() => openPreferences(pluginPreferencesLocation(activePlugin))} autofocus /></div>
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
            if (activeSessionId) openFile(activeSessionId, path);
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
            <p class="text-[11px] text-t3">Local sessions and running chat turns don't survive app quit.</p>
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
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] bg-accent-bg" onclick={() => { showNewItemModal = false; openNewSessionForm(); }}>
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
        activeFilePath={explorerActiveFile}
        modifiedPaths={explorerModifiedPaths}
        onOpenFile={(path) => { if (activeSessionId) openFile(activeSessionId, path); }}
        onPinFile={(path) => { if (activeSessionId) openFile(activeSessionId, path); }}
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
        onOpenPreferences={() => openPreferences(pluginPreferencesLocation(modalPlugin))}
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

<!-- Workspace prompts would sit on top of setup and act on the hidden workspace; they return after it. -->
{#if showOnboarding}
  <Onboarding />
{:else}
  {#each interactionPluginContributions as { plugin, contribution } (`${plugin.id}:${contribution.id}`)}
    <div class="pointer-events-none fixed inset-0 z-[90]" data-plugin-interaction-host={`${plugin.id}:${contribution.id}`}>
      <PluginContributionHost {plugin} {contribution} onNavigate={openPluginContribution} onClose={leavePluginWorkspace} onOpenPreferences={() => openPreferences(pluginPreferencesLocation(plugin))} />
    </div>
  {/each}
  <UpdateToast />
{/if}
<!-- Stays visible during setup: its countdown keeps running and acts when it ends. -->
<PostMergePrompt />
