<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { listen } from "@tauri-apps/api/event";
  import { sessions as sessionsApi, pr as prApi, pty, notify, sessionLogs } from "./lib/api";
  import type { Session, Project } from "./lib/types";
  import { focusEditor, focusTerminal, refocusTerminal, focusExplorer, focusSidebar, getActiveZone, toggleExplorerFocus } from "./lib/focus.svelte";
  import * as projectStore from "./lib/project-store.svelte";
  import * as taskStore from "./lib/task-store.svelte";
  import { installKeyboardRouter, matchChord, MOD_LABEL, isPlatformMod, MOD_ENTER_HINT } from "./lib/keyboard";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import * as navCycle from "./lib/session-nav-cycle.svelte";
  import { computeSidebarSessionOrder, isLoopId, parseLoopId } from "./lib/sidebar-session-order";
  import { isTerminal, isActive as isLoopActive } from "./lib/loop-status";
  import { loadSettings, getSettings, isDark } from "./lib/settings.svelte";
  import { createFormKeyboardController } from "./lib/form-keyboard.svelte";
  import { loadTheme } from "./lib/theme-loader";
  import { startPolling as startCiPolling, getCiChecks, classifyCheck } from "./lib/ci-checks.svelte";
  import { startPolling as startPrCommentPolling } from "./lib/pr-comments.svelte";
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
  import { Input, Label, Button, Checkbox } from "./components/ui";
  import LogViewer from "./components/LogViewer.svelte";
  import PrPanel from "./components/PrPanel.svelte";
  import PostMergePrompt from "./components/PostMergePrompt.svelte";
  import LoopForm from "./components/LoopForm.svelte";
  import LoopDashboard from "./components/LoopDashboard.svelte";
  import PluginContributionHost from "./components/PluginContributionHost.svelte";
  import type { PluginInventory, PluginUiContribution } from "./lib/types";
  import * as loopStore from "./lib/loop-store.svelte";
  import { loops as loopsApi, plugins as pluginsApi } from "./lib/api";
  import { focusMergePrompt, getPrompt } from "./lib/post-merge-prompt.svelte";
  import { getTabs, getActiveTabIndex, addTab } from "./lib/session-tabs.svelte";
  import { isMounted as poolIsMounted, touchMru } from "./lib/mru.svelte";
  import * as orchestrator from "./lib/session-orchestrator.svelte";
  import UpdateToast from "./components/UpdateToast.svelte";
  import { initUpdateListener, focusUpdateToast, getUpdateState } from "./lib/updater.svelte";
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

  // PR form state
  let showPrForm = $state(false);
  let showPrPanel = $state(false);
  let prTitle = $state("");
  let prBody = $state("");
  let prBaseBranch = $state("");
  let prDraft = $state(false);
  let prSubmitting = $state(false);
  let prError = $state("");
  let prFormWrapper = $state<HTMLDivElement | null>(null);
  let prLinkUrl = $state("");
  let prShowLinkField = $state(false);
  let prLinking = $state(false);
  let prRefreshing = $state(false);

  const prFk = createFormKeyboardController(
    () => [
      { key: "t", ref: () => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-title'] input") ?? null },
      { key: "b", ref: () => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-body'] textarea") ?? null },
      { key: "a", ref: () => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-base'] input") ?? null },
      { key: "d", toggle: () => { prDraft = !prDraft; } },
      { key: "r", toggle: () => { if (!prShowLinkField) refreshPr(); } },
    ],
    { wrapper: () => prFormWrapper, onDismiss: () => { showPrForm = false; tick().then(() => refocusTerminal()); } },
  );

  $effect(() => { if (showPrForm && prFormWrapper) prFormWrapper.focus(); });

  function togglePrPanel() {
    const s = sessions.find(x => x.id === activeSessionId);
    if (s?.pr_url) { showPrPanel = !showPrPanel; if (!showPrPanel) tick().then(() => refocusTerminal()); }
    else if (activeSessionId) { openPrForm(); }
  }

  async function openPrForm() {
    if (!activeSessionId) return;
    prError = "";
    prSubmitting = false;
    prShowLinkField = false;
    prLinkUrl = "";
    prLinking = false;
    prRefreshing = false;
    try {
      const defaults = await prApi.generateDefaults(activeSessionId);
      prTitle = defaults.title;
      prBody = defaults.body;
      prBaseBranch = defaults.base_branch;
      prDraft = false;
      showPrForm = true;
    } catch (e: any) {
      showSnackbar(e.toString());
    }
  }

  async function submitPr() {
    if (prSubmitting || !activeSessionId) return;
    prSubmitting = true;
    prError = "";
    try {
      const url = await prApi.create(activeSessionId, prTitle, prBody, prBaseBranch, prDraft);
      showPrForm = false;
      showSnackbar(`PR created: ${url}`, "success");
      await orchestrator.loadSessions();
    } catch (e: any) {
      prError = e.toString();
    } finally {
      prSubmitting = false;
    }
  }

  async function refreshPr() {
    if (prRefreshing || !activeSessionId) return;
    prRefreshing = true;
    prError = "";
    try {
      const result = await prApi.fetchPrUrl(activeSessionId);
      // If result is a real PR URL (contains /pull/), it was found
      if (result && result.includes("/pull/")) {
        showPrForm = false;
        showPrPanel = true;
        showSnackbar("PR linked", "success");
        await orchestrator.loadSessions();
      } else {
        // No PR found — show the paste field and autofocus it
        prShowLinkField = true;
        tick().then(() => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-link'] input")?.focus());
      }
    } catch (e: any) {
      prError = e.toString();
      prShowLinkField = true;
      tick().then(() => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-link'] input")?.focus());
    } finally {
      prRefreshing = false;
    }
  }

  async function linkPr() {
    if (prLinking || !activeSessionId || !prLinkUrl.trim()) return;
    prLinking = true;
    prError = "";
    try {
      await prApi.linkPrUrl(activeSessionId, prLinkUrl.trim());
      showPrForm = false;
      showPrPanel = true;
      showSnackbar("PR linked", "success");
      await orchestrator.loadSessions();
    } catch (e: any) {
      prError = e.toString();
    } finally {
      prLinking = false;
    }
  }

  let logViewerEnabled = $state(false);
  let sessionToDelete = $state<Session | null>(null);
  let projectToDelete = $state<Project | null>(null);
  let projectToEdit = $state<Project | null>(null);
  let loopToDelete = $state<import("./lib/types").LoopRunSummary | null>(null);
  let renamingSessionId = $state<string | null>(null);
  let taskPrefill = $state<{ key: string; title: string; description: string; branch: string; name: string; prompt: string; baseBranch?: string; projectId?: string | null } | null>(null);

  let editorBindRefs = $state<Record<string, EditorTab>>({});
  $effect(() => { for (const [id, ref] of Object.entries(editorBindRefs)) { if (ref) orchestrator.registerEditorRef(id, ref); } });

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
  const activeLoopId = $derived(loopStore.getActiveLoopId());
  const activePlugin = $derived(pluginInventory.find((plugin) => plugin.id === activePluginId) ?? null);
  const activeContribution = $derived(activePlugin?.ui_contributions.find((contribution) => contribution.id === activeContributionId) ?? null);
  const comparePluginContribution = (left: { plugin: PluginInventory; contribution: PluginUiContribution }, right: { plugin: PluginInventory; contribution: PluginUiContribution }) =>
    (left.contribution.order ?? 0) - (right.contribution.order ?? 0) || left.plugin.name.localeCompare(right.plugin.name) || left.plugin.id.localeCompare(right.plugin.id) || left.contribution.id.localeCompare(right.contribution.id);
  const sidebarPluginContributions = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => ["sidebar.header", "sidebar.navigation", "sidebar.section", "sidebar.footer"].includes(contribution.placement)).map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const mainPaneCommands = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => contribution.placement === "main-pane").map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const interactionPluginContributions = $derived(
    pluginInventory.filter((plugin) => plugin.state === "running").flatMap((plugin) =>
      plugin.ui_contributions.filter((contribution) => contribution.placement === "interaction").map((contribution) => ({ plugin, contribution })),
    ).sort(comparePluginContribution),
  );
  const activeProjectName = $derived(activeSession ? (projects.find((p) => p.id === activeSession.project_id)?.name ?? null) : null);
  const activeSessionName = $derived(activeSession ? (activeSession.name || activeSession.branch) : null);
  const ciStatus = $derived.by(() => {
    if (!activeSessionId) return null;
    const checks = getCiChecks(activeSessionId);
    if (checks.length === 0) return null;
    if (checks.some((c) => classifyCheck(c) === "fail")) return "failing" as const;
    if (checks.every((c) => classifyCheck(c) !== "pending")) return "passing" as const;
    return "pending" as const;
  });

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
    const extra: { index: number; label: string; icon?: string; modified?: boolean }[] = [];
    if (diffTabOpen[activeSessionId]) extra.push({ index: -1, label: diffFileName[activeSessionId] || "Diff", icon: "git-compare" });
    if (editorTabOpen[activeSessionId]) extra.push({ index: -2, label: editorFileName[activeSessionId] || "Editor", icon: "file", modified: editorModified[activeSessionId] || false });
    return [...shellTabs, ...extra];
  });

  // ─── Split tree ─────────────────────────────────────────────────────────────
  const splitTreeNode = $derived(splitTree.getTree());
  const hasMultiplePanes = $derived(splitTreeNode !== null && splitTreeNode.type === "split");
  const titlebarActiveTabIdx = $derived.by(() => {
    const tree = splitTree.getTree();
    if (tree?.type === "leaf") {
      const idx = tree.tabs.findIndex((t) => t.ptyKey === tree.activeTab);
      return idx >= 0 ? idx : 0;
    }
    return orchestrator.getUnifiedActiveIndex();
  });

  // Initialize tree when sessions first load (single leaf with all session IDs)
  let splitTreeInitialized = $state(false);

  $effect(() => {
    if (!splitTreeInitialized && sessions.length > 0) {
      splitTreeInitialized = true;
    }
  });

  // ─── Per-session split layout (DB-backed) ───────────────────────────────────
  let lastTreeSessionId = $state<string | null>(null);
  let loadGeneration = 0; // not reactive - just a counter for staleness
  let loadingLayout = $state(false); // suppress auto-save and stale-tab cleanup during load

  // When active session changes, save current tree and load/create for new session
  $effect(() => {
    if (!activeSessionId || !splitTreeInitialized) return;
    if (activeSessionId === lastTreeSessionId) return;

    const tree = splitTree.getTree();
    if (!tree) {
      loadLayoutForSession(activeSessionId);
      return;
    }

    // Check if the active session is already in the tree
    const allLeaves = splitTree.getAllLeaves();
    const hasActive = allLeaves.some((leaf) =>
      leaf.tabs.some((t) => t.ptyKey === activeSessionId)
    );
    if (hasActive) {
      splitTree.focusTab(activeSessionId);
      lastTreeSessionId = activeSessionId;
      return;
    }

    // Session changed — save current tree, then load new
    if (lastTreeSessionId) {
      saveSplitTreeToDb();
    }

    loadLayoutForSession(activeSessionId);
  });

  async function loadLayoutForSession(sessionId: string): Promise<void> {
    loadingLayout = true;
    const gen = ++loadGeneration;
    try {
      const layoutJson = await sessionsApi.getLayout(sessionId);
      // Staleness check - if session changed while we were loading, discard
      if (gen !== loadGeneration) { loadingLayout = false; return; }
      if (layoutJson) {
        const data = JSON.parse(layoutJson);
        if (isValidSerializedTree(data)) {
          splitTree.deserialize(data);
          lastTreeSessionId = sessionId;
          loadingLayout = false;
          return;
        }
      }
    } catch (e) {
      console.warn("Failed to load layout for session", sessionId, e);
    }
    // Staleness check again
    if (gen !== loadGeneration) { loadingLayout = false; return; }
    // No saved layout or invalid - initialize fresh
    const entries = buildTabEntriesForSession(sessionId);
    if (!splitTree.replaceRootLeafTabs(entries)) {
      // Tree is a split (multi-pane) or null — must create fresh
      splitTree.initTree(entries);
    }
    lastTreeSessionId = sessionId;
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

  /** Build TabEntry[] for a session from its current tabs in session-tabs store */
  function buildTabEntriesForSession(sessionId: string): import("./lib/split-tree.svelte").TabEntry[] {
    const session = sessions.find((s) => s.id === sessionId);
    const sessionTabs = getTabs(sessionId);
    if (sessionTabs.length === 0) {
      return [{ ptyKey: sessionId, label: session?.name || session?.branch || "Agent", icon: session?.provider ? "bot" : "terminal", type: "agent" }];
    }
    return sessionTabs.map((t) => ({
      ptyKey: t.index === 0 ? sessionId : `${sessionId}:${t.index}`,
      label: t.index === 0 ? (session?.name || session?.branch || "Agent") : (t.customTitle ? t.label : "Shell"),
      icon: t.index === 0 ? (session?.provider ? "bot" : "terminal") : "terminal",
      type: (t.index === 0 ? "agent" : "shell") as "agent" | "shell",
      customTitle: t.customTitle,
    }));
  }

  // Stale tabs are cleaned up reactively by the $effect below

  // Remove stale tabs when sessions are deleted/archived
  $effect(() => {
    if (loadingLayout) return;
    const sessionIds = new Set(sessions.map((s) => s.id));
    // Collect stale keys: tabs whose session was deleted, OR tabs that don't
    // belong to the current layout's session (cross-contamination guard)
    const allLeaves = splitTree.getAllLeaves();
    const stalePtyKeys: string[] = [];
    for (const leaf of allLeaves) {
      for (const tab of leaf.tabs) {
        const sid = ptyKeyToSessionId(tab.ptyKey);
        if (!sid) continue;
        // Session deleted
        if (!sessionIds.has(sid)) {
          stalePtyKeys.push(tab.ptyKey);
        }
        // Session exists but belongs to a different session (cross-project contamination)
        else if (lastTreeSessionId && sid !== lastTreeSessionId && tab.type === "agent") {
          stalePtyKeys.push(tab.ptyKey);
        }
      }
    }
    for (const key of stalePtyKeys) {
      splitTree.removeSessionFromLeaf(key);
    }
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

  // Get tab info for a leaf — returns Tab[] compatible with TabStrip
  function getLeafTabInfo(leaf: LeafNode): import("./lib/session-tabs.svelte").Tab[] {
    return leaf.tabs.map((tabEntry, i) => ({
      index: i,
      label: tabEntry.label,
      icon: tabEntry.icon,
      customTitle: tabEntry.customTitle,
    }));
  }

  /** Extract the session ID from a pty key (strips ":tabIndex" suffix if present) */
  function ptyKeyToSessionId(ptyKey: string): string {
    const colonIdx = ptyKey.indexOf(":");
    return colonIdx === -1 ? ptyKey : ptyKey.slice(0, colonIdx);
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

    const newLeafId = splitTree.splitFocusedLeaf(direction);
    if (!newLeafId) return;

    // Create a new shell tab within the current session
    const tabIndex = addTab(activeSessionId);
    if (tabIndex === -1) {
      // Undo the split — destroy the empty leaf
      splitTree.destroyLeaf(newLeafId);
      return;
    }
    pty.incrementTabCount(activeSessionId);

    // The pty key for shell tabs is "sessionId:tabIndex"
    const ptyKey = `${activeSessionId}:${tabIndex}`;

    // Verify this ptyKey isn't already in the tree (defensive)
    const existing = splitTree.getLeafForSession(ptyKey);
    if (existing) {
      splitTree.destroyLeaf(newLeafId);
      return;
    }

    splitTree.addSessionToLeaf(newLeafId, { ptyKey, label: "Shell", icon: "terminal", type: "shell" });
    // Wait for Terminal to mount + open before refocusing
    tick().then(() => requestAnimationFrame(() => refocusTerminal()));
  }

  /** Open a new shell tab in the focused split leaf. */
  function splitNewTab(): void {
    if (!activeSessionId) return;
    const focusedLeafId = splitTree.getFocusedLeafId();
    if (!focusedLeafId) return;

    const tabIndex = addTab(activeSessionId);
    if (tabIndex === -1) return;
    pty.incrementTabCount(activeSessionId);

    const ptyKey = `${activeSessionId}:${tabIndex}`;
    splitTree.addSessionToLeaf(focusedLeafId, { ptyKey, label: "Shell", icon: "terminal", type: "shell" });
    refocusTerminal();
  }

  /** Close the active tab in the focused split leaf. */
  function splitCloseTab(): void {
    const leaf = splitTree.getFocusedLeaf();
    if (!leaf || leaf.tabs.length === 0) return;

    const activeEntry = splitTree.getActiveTabEntry(leaf);
    if (!activeEntry) return;

    // Agent tabs can't be closed directly
    if (activeEntry.type === "agent") {
      if (splitTree.getAllLeaves().length > 1) {
        splitTree.closeSplit(leaf.id);
        syncFocusedLeafToOrchestrator();
        tick().then(() => refocusTerminal());
      }
      return;
    }

    // Diff and editor tabs — just remove from tree
    if (activeEntry.type === "diff" || activeEntry.type === "editor") {
      splitTree.removeSessionFromLeaf(activeEntry.ptyKey);
      tick().then(() => refocusTerminal());
      return;
    }

    // Shell tabs — remove from tree + close backend PTY
    splitTree.removeSessionFromLeaf(activeEntry.ptyKey);
    const colonIdx = activeEntry.ptyKey.indexOf(":");
    if (colonIdx !== -1) {
      const sessionId = activeEntry.ptyKey.slice(0, colonIdx);
      const tabIndex = parseInt(activeEntry.ptyKey.slice(colonIdx + 1), 10);
      if (!isNaN(tabIndex)) orchestrator.closeShellTab(sessionId, tabIndex);
    }
    tick().then(() => refocusTerminal());
  }

  /** Navigate to the next tab in the focused split leaf. */
  function splitNextTab(): void {
    const leaf = splitTree.getFocusedLeaf();
    if (!leaf || leaf.tabs.length <= 1) return;
    const currentIdx = leaf.tabs.findIndex((t) => t.ptyKey === leaf.activeTab);
    const nextIdx = (currentIdx + 1) % leaf.tabs.length;
    splitTree.setLeafActiveTab(leaf.id, leaf.tabs[nextIdx].ptyKey);
  }

  /** Navigate to the previous tab in the focused split leaf. */
  function splitPrevTab(): void {
    const leaf = splitTree.getFocusedLeaf();
    if (!leaf || leaf.tabs.length <= 1) return;
    const currentIdx = leaf.tabs.findIndex((t) => t.ptyKey === leaf.activeTab);
    const prevIdx = (currentIdx - 1 + leaf.tabs.length) % leaf.tabs.length;
    splitTree.setLeafActiveTab(leaf.id, leaf.tabs[prevIdx].ptyKey);
  }

  /** Toggle diff tab: if it exists in the tree, focus it; otherwise add it to focused leaf. */
  function toggleDiffInTree(): void {
    if (!activeSessionId) return;
    const diffPtyKey = `${activeSessionId}:diff`;

    // If already open, toggle: if active focus away, if not active focus it
    const existing = splitTree.findTab(diffPtyKey);
    if (existing) {
      if (existing.leaf.activeTab === diffPtyKey) {
        // Diff is active — close it
        splitTree.removeSessionFromLeaf(diffPtyKey);
        tick().then(() => refocusTerminal());
      } else {
        // Diff exists but not active — focus it
        splitTree.focusTab(diffPtyKey);
      }
      return;
    }

    // Add diff tab to focused leaf
    const focusedLeafId = splitTree.getFocusedLeafId();
    if (!focusedLeafId) return;
    const tabEntry: import("./lib/split-tree.svelte").TabEntry = {
      ptyKey: diffPtyKey,
      label: "Diff",
      icon: "git-compare",
      type: "diff",
    };
    splitTree.addSessionToLeaf(focusedLeafId, tabEntry);
  }

  /** Open a file in an editor tab. If already open, focus it. */
  function openFileInTree(sessionId: string, filePath: string): void {
    // Reject traversal paths (.. as path segment)
    if (filePath.split(/[/\\]/).includes("..")) return;
    const editorPtyKey = `${sessionId}:editor:${filePath}`;

    // If already open, focus it
    if (splitTree.focusTab(editorPtyKey)) return;

    // Add editor tab to focused leaf
    const focusedLeafId = splitTree.getFocusedLeafId();
    if (!focusedLeafId) return;
    const fileName = filePath.split("/").pop() ?? filePath;
    const tabEntry: import("./lib/split-tree.svelte").TabEntry = {
      ptyKey: editorPtyKey,
      label: fileName,
      icon: "file",
      type: "editor",
      filePath,
    };
    splitTree.addSessionToLeaf(focusedLeafId, tabEntry);
  }

  // Sync the focused leaf's active session to the orchestrator
  function syncFocusedLeafToOrchestrator(): void {
    const leaf = splitTree.getFocusedLeaf();
    if (leaf && leaf.tabs.length > 0) {
      const activeEntry = splitTree.getActiveTabEntry(leaf);
      if (activeEntry) {
        const sessionId = ptyKeyToSessionId(activeEntry.ptyKey);
        if (sessionId !== activeSessionId) {
          selectWorkspaceSession(sessionId);
        }
      }
    }
  }

  // ─── Split tree persistence (DB) ────────────────────────────────────────────
  let splitSaveTimeout: ReturnType<typeof setTimeout> | null = null;

  function saveSplitTreeToDb(): void {
    const data = splitTree.serialize();
    if (!data || !lastTreeSessionId) return;
    const sessionId = lastTreeSessionId;
    sessionsApi.saveLayout(sessionId, JSON.stringify(data)).catch((e) => {
      console.warn("Failed to save split layout for session", sessionId, e);
    });
  }

  // Auto-save split tree on changes (debounced 500ms)
  $effect(() => {
    const _tree = splitTree.getTree();
    if (splitTreeInitialized && _tree && lastTreeSessionId && !loadingLayout) {
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

  function focusPluginInteraction(): boolean {
    const interaction = document.querySelector<HTMLElement>("[data-plugin-interaction-host] [data-plugin-ui-contribution]");
    if (!interaction) return false;
    interaction.focus();
    return true;
  }

  function invalidatePluginPage(pluginId: string): void {
    if (activePluginId === pluginId) leavePluginWorkspace();
  }

  function openPluginContribution(pluginId: string, contributionId: string): void {
    const plugin = pluginInventory.find((candidate) => candidate.id === pluginId && candidate.state === "running");
    const contribution = plugin?.ui_contributions.find((candidate) => candidate.id === contributionId && candidate.placement === "main-pane");
    if (!plugin || !contribution) {
      showSnackbar("Plugin main-pane contribution is unavailable");
      return;
    }
    if (activePluginId === pluginId && activeContributionId === contributionId) return;
    loopStore.setActiveLoopId(null);
    activePluginId = pluginId;
    activeContributionId = contributionId;
  }

  function selectWorkspaceSession(sessionId: string): void {
    leavePluginWorkspace();
    loopStore.setActiveLoopId(null);
    orchestrator.selectSession(sessionId);
  }

  function jumpToWorkspaceSession(index: number): void {
    leavePluginWorkspace();
    loopStore.setActiveLoopId(null);
    orchestrator.jumpToSession(index);
  }

  function selectWorkspaceLoop(loopId: string): void {
    leavePluginWorkspace();
    loopStore.setActiveLoopId(loopId);
    touchMru(`loop:${loopId}`);
  }

  // ─── Lifecycle ──────────────────────────────────────────────────────────────

  /** Valid IDs for MRU cycling — includes session IDs + loop:<id> entries. */
  function getSwitchableIds(): Set<string> {
    const ids = orchestrator.getSwitchableSessionIds();
    for (const p of projects) {
      for (const loop of loopStore.getLoopsForProject(p.id)) {
        if (loop.status !== "draft" && !isTerminal(loop.status)) {
          ids.add(`loop:${loop.id}`);
        }
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
    void refreshPlugins();

    const cleanupEvents = orchestrator.startEventListeners();
    const cleanupSymphony = orchestrator.startSymphonyPolling();
    const cleanupCi = startCiPolling(orchestrator.getSessions());
    const cleanupPrComments = startPrCommentPolling(orchestrator.getSessions());
    const cleanupLoopListener = loopStore.startLoopEventListener(() => projectStore.getProjects().map((p) => p.id));
    const unlistenSettings = listen("settings-changed", () => { loadSettings().then(() => loadTheme()); });
    const unlistenCleanup = listen<string>("cleanup-error", (event) => { showSnackbar(event.payload); });
    const unlistenPluginRuntime = listen<import("./lib/types").PluginInventory>("plugin-runtime-changed", (event) => {
      pluginInventory = pluginInventory.filter((plugin) => plugin.id !== event.payload.id).concat(event.payload);
      if (event.payload.state !== "running") invalidatePluginPage(event.payload.id);
    });

    initUpdateListener();
    const onPluginShortcut = (event: KeyboardEvent) => {
      if (event.defaultPrevented || !isPlatformMod(event) || matchChord(event)) return;
      const key = /^Key[A-Z]$/.test(event.code) ? event.code.slice(3) : event.key.toUpperCase();
      const parts = ["Mod", ...(event.shiftKey ? ["Shift"] : []), ...(event.altKey ? ["Alt"] : []), key];
      const shortcut = parts.join("+");
      const target = mainPaneCommands.find(({ contribution }) => contribution.shortcut === shortcut);
      if (target) { event.preventDefault(); openPluginContribution(target.plugin.id, target.contribution.id); }
    };
    window.addEventListener("keydown", onPluginShortcut);

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
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeSessionId ?? undefined;
          if (!sw.isCycling) startCycle(currentId, getSwitchableIds());
          else advance(1);
        } else if (action.type === "tab_switch_reverse") {
          const sw = getCycleState();
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeSessionId ?? undefined;
          if (!sw.isCycling) { startCycle(currentId, getSwitchableIds()); advance(-1); }
          else advance(-1);
        } else if (action.type === "focus_terminal") {
          if (getCycleState().isCycling) cancel();
          if (navCycle.isCycling()) navCycle.cancel();
          showSessionForm = false; showProjectForm = false; projectToEdit = null; showShortcuts = false; showNewItemModal = false; showTaskForm = false; showPrForm = false; showPrPanel = false; showLoopForm = false; sessionToDelete = null; commandMenuOpen = false; commandMenuFileMode = false;
        } else if (action.type === "command_palette") { commandMenuOpen = !commandMenuOpen; }
        else if (action.type === "open_preferences") { openPreferences(); }
        else if (action.type === "show_shortcuts") { showShortcuts = !showShortcuts; }
        else if (action.type === "new_tab") { splitNewTab(); }
        else if (action.type === "close_tab") { splitCloseTab(); }
        else if (action.type === "next_tab") { splitNextTab(); }
        else if (action.type === "prev_tab") { splitPrevTab(); }
        else if (action.type === "next_session") {
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeSessionId ?? undefined;
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, currentId, 1);
          else navCycle.advance(1);
        } else if (action.type === "prev_session") {
          const currentId = activeLoopId ? `loop:${activeLoopId}` : activeSessionId ?? undefined;
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
        else if (action.type === "save_file") { orchestrator.saveActiveEditor(); }
        else if (action.type === "toggle_pr_panel") { togglePrPanel(); }
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
      () => !showSessionForm && !showProjectForm && !commandMenuOpen && !showShortcuts && !showNewItemModal && !showTaskForm && !showPrForm && !showPrPanel && !showLoopForm && !getCycleState().isCycling && !navCycle.isCycling(),
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
        else if (e.key === 's' || e.key === 't') { showNewItemModal = false; showTaskForm = true; }
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

    /** Route a navigation target (may be a session ID or a loop:<id> prefixed string). */
    function routeNavTarget(target: string): void {
      if (isLoopId(target)) {
        selectWorkspaceLoop(parseLoopId(target));
      } else {
        selectWorkspaceSession(target);
      }
    }

    function onKeyUp(e: KeyboardEvent) {
      const isModRelease = (e.key === "Control" && !e.ctrlKey) || (e.key === "Meta" && !e.metaKey);
      if (!isModRelease) return;
      if (getCycleState().isCycling) { const target = commit(); if (target) { routeNavTarget(target); if (!isLoopId(target)) focusTerminal(); } }
      if (navCycle.isCycling()) { const target = navCycle.commit(); if (target) { routeNavTarget(target); if (!isLoopId(target)) focusTerminal(); } }
    }
    function onBlur() { setTimeout(() => { if (!document.hasFocus()) { if (getCycleState().isCycling) cancel(); if (navCycle.isCycling()) navCycle.cancel(); } }, 0); }
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);

    return () => { window.removeEventListener("keydown", onPluginShortcut); cleanup(); cleanupEvents(); cleanupSymphony(); cleanupCi(); cleanupPrComments(); cleanupLoopListener(); unlistenSettings.then((fn) => fn()); unlistenCleanup.then((fn) => fn()); unlistenPluginRuntime.then((fn) => fn()); unlistenClose.then((fn) => fn()); window.removeEventListener("keydown", onModalKeydown, true); window.removeEventListener("keyup", onKeyUp); window.removeEventListener("blur", onBlur); };
  });
</script>

<main class="flex flex-col h-screen">
  <Titlebar
    projectName={activeProjectName}
    sessionName={activeSessionName}
    {sidebarVisible}
    prUrl={sessions.find(s => s.id === activeSessionId)?.pr_url ?? null}
    prState={sessions.find(s => s.id === activeSessionId)?.pr_state ?? null}
    {ciStatus}
    hasChanges={!!activeSessionId}
    sessionId={activeSessionId}
    tabs={hasMultiplePanes ? [] : titlebarTabs}
    activeTabIndex={titlebarActiveTabIdx}
    runningCount={sessions.filter(s => s.status === 'active').length}
    activeProvider={activeSession?.provider ?? null}
    onSelectTab={(i) => { leavePluginWorkspace(); loopStore.setActiveLoopId(null); const tree = splitTree.getTree(); if (tree?.type === "leaf" && tree.tabs[i]) splitTree.setLeafActiveTab(tree.id, tree.tabs[i].ptyKey); else orchestrator.selectUnifiedTab(i); }}
    onCloseTab={(i) => {
      if (!activeSessionId) return;
      if (i === -1) orchestrator.closeDiffTab(activeSessionId);
      else if (i === -2) orchestrator.closeEditorTab(activeSessionId);
      else { orchestrator.closeShellTab(activeSessionId, i); }
    }}
    onAddTab={() => orchestrator.handleNewTab()}
    onCreatePr={openPrForm}
    onOpenCommand={() => { commandMenuFileMode = false; commandMenuOpen = true; }}
    onTogglePrPanel={togglePrPanel}
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
        onPickTask={(task, repoPath) => { const proj = projects.find(p => p.path === repoPath); taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: `${task.key}: ${task.title}`, prompt: "", baseBranch: task.base_branch, projectId: proj?.id ?? null }; showSessionForm = true; }}
        onAddProject={openAddProject}
        onOpenPreferences={openPreferences}
        onCreateSession={() => { showTaskForm = true; }}
        onSessionsChanged={() => { orchestrator.loadSessions(); taskStore.refresh(projects.map((p) => p.path)); }}
        onSelectLoop={selectWorkspaceLoop}
        onStartLoop={(id) => { loopsApi.start(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onTickLoop={(id) => { loopsApi.tick(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onStopLoop={(id) => { loopsApi.stop(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onDeleteLoop={(id) => { const loop = projects.flatMap(p => loopStore.getLoopsForProject(p.id)).find(l => l.id === id); if (!loop) return; const hasSessions = (loopStore.getSessionsForLoop(id) ?? []).length > 0; if (hasSessions) { loopToDelete = loop; } else { deleteLoopOnly(id); } }}
        onDeleteLoopSession={(session, loopId) => { const loop = projects.flatMap(p => loopStore.getLoopsForProject(p.id)).find(l => l.id === loopId); if (loop && isLoopActive(loop.status)) { showSnackbar("Stop the loop before deleting its sessions"); } else { orchestrator.deleteSession(session); } }}
        selectedLoopId={activeLoopId}
        onToggleDiff={toggleDiffInTree}
        pluginContributions={sidebarPluginContributions}
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
        onCreated={(session) => { leavePluginWorkspace(); showSessionForm = false; orchestrator.createSession(session); focusTerminal(); }}
        onCancel={() => { showSessionForm = false; taskPrefill = null; tick().then(() => refocusTerminal()); }}
      />
    </FormDialog>
    {/if}

    {#if showTaskForm}
    <FormDialog title="New Task" onClose={() => { showTaskForm = false; tick().then(() => refocusTerminal()); }}>
      <TaskForm
        mode="create"
        {projects}
        {sessions}
        tasks={taskStore.getAllTasks()}
        onSubmitted={() => { showTaskForm = false; taskStore.refresh(projects.map((p) => p.path)); focusTerminal(); }}
        onCancel={() => { showTaskForm = false; tick().then(() => refocusTerminal()); }}
        onSessionCreated={(session) => { leavePluginWorkspace(); showTaskForm = false; orchestrator.createSession(session); focusTerminal(); }}
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
      onPickTask={(task) => { taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: `${task.key}: ${task.title}`, prompt: "" }; showSessionForm = true; }}
      onCreateTask={() => { showTaskForm = true; }}
      onToggleDiff={() => toggleDiffInTree()}
      onOpenFile={(path) => { if (activeSessionId) openFileInTree(activeSessionId, path); }}
      onOpenLogViewer={logViewerEnabled ? () => { showLogViewer = true; } : undefined}
      onCreatePr={openPrForm}
      onSplitVertical={() => handleSplitAction("split_vertical")}
      onSplitHorizontal={() => handleSplitAction("split_horizontal")}
      onCloseSplit={() => handleSplitAction("close_split")}
      pluginCommands={mainPaneCommands}
      onOpenPluginContribution={openPluginContribution}
    />

    <KeyboardShortcuts open={showShortcuts} onOpenChange={(v) => (showShortcuts = v)} />

    <!-- Split leaf snippet: renders a leaf pane with its own tab bar and terminal -->
    {#snippet splitLeafSnippet(leaf: LeafNode)}
      {@const leafTabs = getLeafTabInfo(leaf)}
      {@const activeEntry = splitTree.getActiveTabEntry(leaf)}
      {@const activeTabIdx = leaf.tabs.findIndex((t) => t.ptyKey === leaf.activeTab)}
      {@const showLeafTabBar = hasMultiplePanes}
      <div
        class="split-leaf {hasMultiplePanes ? '' : 'split-leaf-single'}"
        class:split-leaf-focused={leaf.id === splitTree.getFocusedLeafId() && hasMultiplePanes}
        role="group"
        aria-label="Split pane"
        onclick={() => { splitTree.setFocusedLeaf(leaf.id); if (activeEntry) { const sid = ptyKeyToSessionId(activeEntry.ptyKey); selectWorkspaceSession(sid); } focusTerminal(); }}
      >
        {#if showLeafTabBar}
        <div class="flex items-stretch h-[38px] bg-chrome border-b border-border shrink-0">
          <TabStrip
            tabs={leafTabs}
            activeTabIndex={activeTabIdx >= 0 ? activeTabIdx : 0}
            focused={leaf.id === splitTree.getFocusedLeafId()}
            showAddButton={true}
            showCloseButton={hasMultiplePanes}
            draggable={hasMultiplePanes}
            onSelectTab={(i) => { splitTree.setFocusedLeaf(leaf.id); if (leaf.tabs[i]) splitTree.setLeafActiveTab(leaf.id, leaf.tabs[i].ptyKey); }}
            onAddTab={() => { splitTree.setFocusedLeaf(leaf.id); splitNewTab(); }}
            onClose={() => splitTree.closeSplit(leaf.id)}
            onTabDragStart={(e, tabIndex) => handleTabDragStart(e, leaf.tabs[tabIndex]?.ptyKey ?? "", leaf.id)}
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
                  sessionId={tabEntry.ptyKey}
                  visible={isActiveInLeaf && !activeLoopId && !activePluginId}
                  focused={isActiveInLeaf && sessionId === activeSessionId && !activePluginId && leaf.id === splitTree.getFocusedLeafId() && zone === "terminal" && !showNewItemModal && !sessionToDelete && !showTaskForm && !showProjectForm && !showPrPanel}
                  exited={tabEntry.type === "agent" && session.status === "exited"}
                  skipAttach={tabEntry.type === "shell"}
                  onAttached={() => { if (tabEntry.type === "agent" && session?.status === "exited") orchestrator.updateSessionStatus(session.id, "active"); if (tabEntry.type === "shell" && leaf.id === splitTree.getFocusedLeafId()) refocusTerminal(); }}
                  onFocused={(event) => {
                    if (event.type === "focusin" && sessionId !== activeSessionId) return;
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

    <!-- Always render through split tree (single leaf = normal view) -->
    {#if splitTreeNode}
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
          <div class="min-h-0 flex-1"><PluginContributionHost plugin={activePlugin} contribution={activeContribution} onNavigate={openPluginContribution} onClose={leavePluginWorkspace} onOpenPreferences={openPreferences} autofocus /></div>
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

    {#if sessions.length === 0 && !showProjectForm && !showSessionForm && !activeLoopId && !activePluginId}
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
      <SharedDialog open={true} onOpenChange={(v) => { if (!v) showNewItemModal = false; }} title="New…" class="w-[268px] rounded-[13px] border-border-s shadow-[0_24px_64px_-14px_rgba(0,0,0,0.55)] overflow-hidden">
        <div>
          <div class="flex items-center px-[15px] pt-[13px] pb-[11px]">
            <span class="text-[13px] font-semibold text-t1">New…</span>
            <span class="ml-auto font-mono text-[10px] text-t3 border border-border rounded-[5px] px-1.5 py-[2px]">esc</span>
          </div>
          <div class="px-2 pb-[9px] flex flex-col gap-[2px]">
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] bg-accent-bg" onclick={() => { showNewItemModal = false; showTaskForm = true; }}>
              <span class="w-[22px] h-[22px] rounded-[7px] flex items-center justify-center font-mono text-[11px] bg-panel-hi text-t2">☰</span>
              <span class="flex-1 text-[13.5px] text-t1">Task</span>
              <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel">t</span>
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

{#if showPrForm}
  <FormDialog title="Create Pull Request" onClose={() => { showPrForm = false; tick().then(() => refocusTerminal()); }}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div bind:this={prFormWrapper} tabindex="-1" onkeydown={prFk.handleKeydown} onfocusin={prFk.handleFocusin} class="outline-none px-5 pb-5" data-form-keyboard>
      <form class="space-y-3" onsubmit={(e) => { e.preventDefault(); submitPr(); }} onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submitPr(); } }}>
        <div class="space-y-1" data-field="pr-title">
          <Label>Title <span class="font-mono text-[10px] px-1 rounded {prFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">T</span></Label>
          <Input bind:value={prTitle} />
        </div>
        <div class="space-y-1" data-field="pr-body">
          <Label>Body <span class="font-mono text-[10px] px-1 rounded {prFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">B</span></Label>
          <textarea bind:value={prBody} rows="10" class="w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 resize-y focus:outline-none focus:ring-1 focus:ring-accent font-mono text-xs"></textarea>
        </div>
        <div class="space-y-1" data-field="pr-base">
          <Label>Base branch <span class="font-mono text-[10px] px-1 rounded {prFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">A</span></Label>
          <Input bind:value={prBaseBranch} />
        </div>
        <div class="flex items-center gap-4">
          <Checkbox id="pr-draft" label="Draft PR" bind:checked={prDraft} tabindex={-1} />
          <span class="font-mono text-[10px] px-1 rounded {prFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">D</span>
        </div>
        <div class="border-t border-border pt-2 space-y-2">
          <button type="button" class="text-xs text-t3 hover:text-accent transition-colors" disabled={prRefreshing} onclick={() => refreshPr()}>
            {prRefreshing ? "Checking…" : "PR already exists? Refresh"} <span class="font-mono text-[10px] px-1 rounded {prFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">R</span>
          </button>
          {#if prShowLinkField}
            <div class="flex gap-2 items-center" data-field="pr-link">
              <Input bind:value={prLinkUrl} placeholder="https://github.com/owner/repo/pull/123" aria-label="PR URL" />
              <Button type="button" variant="primary" disabled={prLinking || !prLinkUrl.trim()} onclick={() => linkPr()}>
                {prLinking ? "Linking…" : "Link"}
              </Button>
            </div>
          {/if}
        </div>
        {#if prError}
          <p class="text-xs text-status-exited">{prError}</p>
        {/if}
        <div class="flex items-center justify-between pt-2 border-t border-border">
          <div class="flex items-center gap-2">
            {#if prFk.mode === "insert"}
              <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-accent-bg text-accent font-medium">INSERT</span>
              <span class="text-[10px] text-t3">esc → normal mode</span>
            {:else}
              <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
              <span class="text-[10px] text-t3">press a key to focus field</span>
            {/if}
          </div>
          <div class="flex gap-2">
            <Button type="button" onclick={() => { showPrForm = false; tick().then(() => refocusTerminal()); }}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={prSubmitting || !prTitle.trim()}>
              {prSubmitting ? "Creating…" : "Create"} <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>
            </Button>
          </div>
        </div>
      </form>
    </div>
  </FormDialog>
{/if}

{#if showPrPanel && activeSession?.pr_url}
  <FormDialog title="Pull Request" onClose={() => { showPrPanel = false; tick().then(() => refocusTerminal()); }}>
    <PrPanel
      sessionId={activeSession.id}
      prUrl={activeSession.pr_url!}
      prState={activeSession.pr_state ?? null}
      sessionName={activeSession.name}
      onClose={() => { showPrPanel = false; tick().then(() => refocusTerminal()); }}
    />
  </FormDialog>
{/if}

{#if getSnackbarMessage()}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed bottom-4 left-4 z-[100] max-w-lg cursor-pointer rounded-lg {getSnackbarType() === 'error' ? 'bg-red-600' : 'bg-green-600'} px-4 py-3 shadow-lg" onclick={() => { navigator.clipboard.writeText(getSnackbarMessage()!); dismissSnackbar(); }} title="Click to copy and dismiss">
    <p class="text-sm text-white font-mono break-all">{getSnackbarMessage()}</p>
    <p class="text-xs {getSnackbarType() === 'error' ? 'text-red-200' : 'text-green-200'} mt-1">Click to dismiss</p>
  </div>
{/if}

{#each interactionPluginContributions as { plugin, contribution } (`${plugin.id}:${contribution.id}`)}
  <div class="pointer-events-none fixed inset-0 z-[90]" data-plugin-interaction-host={`${plugin.id}:${contribution.id}`}>
    <PluginContributionHost {plugin} {contribution} onNavigate={openPluginContribution} onClose={leavePluginWorkspace} onOpenPreferences={openPreferences} />
  </div>
{/each}
<PostMergePrompt />
<UpdateToast />
