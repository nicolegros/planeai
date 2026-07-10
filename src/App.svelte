<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { listen } from "@tauri-apps/api/event";
  import { sessions as sessionsApi, pr as prApi, pty, notify, sessionLogs } from "./lib/api";
  import type { Session, Project } from "./lib/types";
  import { focusTerminal, refocusTerminal, focusExplorer, getActiveZone } from "./lib/focus.svelte";
  import * as projectStore from "./lib/project-store.svelte";
  import * as taskStore from "./lib/task-store.svelte";
  import { installKeyboardRouter, MOD_LABEL, isPlatformMod, MOD_ENTER_HINT } from "./lib/keyboard";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import * as navCycle from "./lib/session-nav-cycle.svelte";
  import { computeSidebarSessionOrder } from "./lib/sidebar-session-order";
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
  import JiraDepartedPrompt from "./components/JiraDepartedPrompt.svelte";
  import LoopForm from "./components/LoopForm.svelte";
  import LoopDashboard from "./components/LoopDashboard.svelte";
  import * as loopStore from "./lib/loop-store.svelte";
  import { loops as loopsApi } from "./lib/api";
  import { focusMergePrompt, getPrompt } from "./lib/post-merge-prompt.svelte";
  import { startListening as startJiraDepartedListening, stopListening as stopJiraDepartedListening, focusDepartedPrompt, getCurrent as getDepartedPrompt } from "./lib/jira-departed-prompt.svelte";
  import { getTabs, getActiveTabIndex } from "./lib/session-tabs.svelte";
  import { isMounted as poolIsMounted } from "./lib/mru.svelte";
  import * as orchestrator from "./lib/session-orchestrator.svelte";

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

  const prFk = createFormKeyboardController(
    () => [
      { key: "t", ref: () => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-title'] input") ?? null },
      { key: "b", ref: () => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-body'] textarea") ?? null },
      { key: "a", ref: () => prFormWrapper?.querySelector<HTMLElement>("[data-field='pr-base'] input") ?? null },
      { key: "d", toggle: () => { prDraft = !prDraft; } },
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
  let logViewerEnabled = $state(false);
  let sessionToDelete = $state<Session | null>(null);
  let projectToDelete = $state<Project | null>(null);
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
  const diffTabActive = $derived(orchestrator.getDiffTabActive());
  const editorTabOpen = $derived(orchestrator.getEditorTabOpen());
  const editorTabActive = $derived(orchestrator.getEditorTabActive());
  const diffFileName = $derived(orchestrator.getDiffFileName());
  const editorFileName = $derived(orchestrator.getEditorFileName());
  const editorModified = $derived(orchestrator.getEditorModified());
  const symphonyStatus = $derived(orchestrator.getSymphonyStatus());
  const zone = $derived(getActiveZone());
  const activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);
  const activeLoopId = $derived(loopStore.getActiveLoopId());
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

  // Session IDs in sidebar display order
  const sidebarSessionOrder = $derived(computeSidebarSessionOrder(projects, sessions, taskStore.getTasksByProject(), !!getSettings().hide_done_tasks));

  // Pre-compute titlebar tabs to avoid IIFE re-evaluation on every render
  const titlebarTabs = $derived.by(() => {
    if (!activeSessionId) return [];
    const shellTabs = getTabs(activeSessionId).map(t => t.index === 0 ? { ...t, label: activeSession?.provider || getSettings().default_provider || "Agent" } : t);
    const extra: { index: number; label: string; icon?: string; modified?: boolean }[] = [];
    if (diffTabOpen[activeSessionId]) extra.push({ index: -1, label: diffFileName[activeSessionId] || "Diff", icon: "git-compare" });
    if (editorTabOpen[activeSessionId]) extra.push({ index: -2, label: editorFileName[activeSessionId] || "Editor", icon: "file", modified: editorModified[activeSessionId] || false });
    return [...shellTabs, ...extra];
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

  async function deleteProject(p: Project) {
    await projectStore.deleteProject(p.id);
    projectToDelete = null;
  }

  // ─── Lifecycle ──────────────────────────────────────────────────────────────
  onMount(() => {
    projectStore.loadProjects().then(() => {
      taskStore.loadTasks(projectStore.getProjects().map((p) => p.path));
      loopStore.refreshAllLoops(projectStore.getProjects().map((p) => p.id));
    });
    orchestrator.loadSessions();
    loadSettings().then(() => loadTheme());

    const cleanupEvents = orchestrator.startEventListeners();
    const cleanupSymphony = orchestrator.startSymphonyPolling();
    const cleanupCi = startCiPolling(orchestrator.getSessions());
    const cleanupPrComments = startPrCommentPolling(orchestrator.getSessions());
    const cleanupLoopListener = loopStore.startLoopEventListener(() => projectStore.getProjects().map((p) => p.id));
    const unlistenSettings = listen("settings-changed", () => { loadSettings().then(() => loadTheme()); });
    const unlistenCleanup = listen<string>("cleanup-error", (event) => { showSnackbar(event.payload); });

    startJiraDepartedListening();

    notify.isInstalled().then((installed) => { if (!installed) showHookPrompt = true; });
    sessionLogs.isEnabled().then((enabled) => { logViewerEnabled = enabled; });
    const unlistenClose = orchestrator.setupQuitGuard((count) => { quitDirectCount = count; showQuitConfirm = true; });

    const cleanup = installKeyboardRouter(
      (action) => {
        if (action.type === "new_session") {
          showNewItemModal = true;
        } else if (action.type === "new_project") { showProjectForm = true; }
        else if (action.type === "toggle_sidebar") { sidebarVisible = !sidebarVisible; }
        else if (action.type === "jump_to_session") { orchestrator.jumpToSession(action.index); }
        else if (action.type === "tab_switch") {
          const sw = getCycleState();
          if (!sw.isCycling) startCycle(activeSessionId ?? undefined, orchestrator.getSwitchableSessionIds());
          else advance(1);
        } else if (action.type === "tab_switch_reverse") {
          const sw = getCycleState();
          if (!sw.isCycling) { startCycle(activeSessionId ?? undefined, orchestrator.getSwitchableSessionIds()); advance(-1); }
          else advance(-1);
        } else if (action.type === "focus_terminal") {
          if (getCycleState().isCycling) cancel();
          if (navCycle.isCycling()) navCycle.cancel();
          showSessionForm = false; showProjectForm = false; showShortcuts = false; showNewItemModal = false; showTaskForm = false; showPrForm = false; showPrPanel = false; showLoopForm = false; sessionToDelete = null; commandMenuOpen = false; commandMenuFileMode = false;
        } else if (action.type === "command_palette") { commandMenuOpen = !commandMenuOpen; }
        else if (action.type === "open_preferences") { openPreferences(); }
        else if (action.type === "show_shortcuts") { showShortcuts = !showShortcuts; }
        else if (action.type === "new_tab") { orchestrator.handleNewTab(); }
        else if (action.type === "close_tab") { orchestrator.handleCloseTab(); }
        else if (action.type === "next_tab") { orchestrator.handleNextTab(); }
        else if (action.type === "prev_tab") { orchestrator.handlePrevTab(); }
        else if (action.type === "next_session") {
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, activeSessionId ?? undefined, 1);
          else navCycle.advance(1);
        } else if (action.type === "prev_session") {
          if (!navCycle.isCycling()) navCycle.startPreview(sidebarSessionOrder, activeSessionId ?? undefined, -1);
          else navCycle.advance(-1);
        }
        else if (action.type === "toggle_diff") { orchestrator.toggleDiff(); }
        else if (action.type === "toggle_file_explorer") { fileExplorerVisible = !fileExplorerVisible; if (fileExplorerVisible) focusExplorer(); else focusTerminal(); }
        else if (action.type === "toggle_task_panel") { if (!sidebarVisible) sidebarVisible = true; }
        else if (action.type === "toggle_sessions_panel") { if (!sidebarVisible) sidebarVisible = true; }
        else if (action.type === "refresh_tasks") { if (!sidebarVisible) sidebarVisible = true; taskStore.refresh(projects.map((p) => p.path)); }
        else if (action.type === "open_file") { commandMenuFileMode = true; commandMenuOpen = true; }
        else if (action.type === "save_file") { orchestrator.saveActiveEditor(); }
        else if (action.type === "toggle_pr_panel") { togglePrPanel(); }
        else if (action.type === "focus_merge_prompt") { if (getPrompt()) focusMergePrompt(); else if (getDepartedPrompt()) focusDepartedPrompt(); }
      },
      () => !showSessionForm && !showProjectForm && !commandMenuOpen && !showShortcuts && !showNewItemModal && !showTaskForm && !showPrForm && !showPrPanel && !showLoopForm && !getCycleState().isCycling && !navCycle.isCycling(),
      () => !!(activeSessionId && editorTabActive[activeSessionId]),
      () => !!document.activeElement?.closest('[data-form-keyboard]'),
    );

    function onModalKeydown(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (showNewItemModal) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape') { showNewItemModal = false; }
        else if (e.key === 's') { showNewItemModal = false; if (projects.length === 0) showProjectForm = true; else showSessionForm = true; }
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
      } else if (showQuitConfirm) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.key === 'Escape' || e.key === 'n') showQuitConfirm = false;
        else if (e.key === 'q' || e.key === 'y') { showQuitConfirm = false; getCurrentWindow().destroy(); }
      }
    }
    window.addEventListener("keydown", onModalKeydown, true);

    function onKeyUp(e: KeyboardEvent) {
      const isModRelease = (e.key === "Control" && !e.ctrlKey) || (e.key === "Meta" && !e.metaKey);
      if (!isModRelease) return;
      if (getCycleState().isCycling) { const target = commit(); if (target) orchestrator.selectSession(target); focusTerminal(); }
      if (navCycle.isCycling()) { const target = navCycle.commit(); if (target) orchestrator.selectSession(target); focusTerminal(); }
    }
    function onBlur() { setTimeout(() => { if (!document.hasFocus()) { if (getCycleState().isCycling) cancel(); if (navCycle.isCycling()) navCycle.cancel(); } }, 0); }
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);

    return () => { cleanup(); cleanupEvents(); cleanupSymphony(); cleanupCi(); cleanupPrComments(); cleanupLoopListener(); stopJiraDepartedListening(); unlistenSettings.then((fn) => fn()); unlistenCleanup.then((fn) => fn()); unlistenClose.then((fn) => fn()); window.removeEventListener("keydown", onModalKeydown, true); window.removeEventListener("keyup", onKeyUp); window.removeEventListener("blur", onBlur); };
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
    tabs={titlebarTabs}
    activeTabIndex={orchestrator.getUnifiedActiveIndex()}
    runningCount={sessions.filter(s => s.status === 'active').length}
    activeProvider={activeSession?.provider ?? null}
    onSelectTab={orchestrator.selectUnifiedTab}
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
        onSelectSession={(id) => { loopStore.setActiveLoopId(null); orchestrator.selectSession(id); }}
        onArchiveSession={(s) => orchestrator.archiveSession(s)}
        onDeleteSession={(s) => (sessionToDelete = s)}
        onRestartSession={(s) => orchestrator.restartSession(s)}
        onRenameSession={doRename}
        onStartRename={(id) => { renamingSessionId = id || null; if (!id) focusTerminal(); }}
        onDeleteProject={(p) => (projectToDelete = p)}
        onPickTask={(task, repoPath) => { const proj = projects.find(p => p.path === repoPath); taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: `${task.key}: ${task.title}`, prompt: "", baseBranch: task.base_branch, projectId: proj?.id ?? null }; showSessionForm = true; }}
        onAddProject={() => (showProjectForm = true)}
        onOpenPreferences={openPreferences}
        onCreateSession={() => { showNewItemModal = true; }}
        onSessionsChanged={() => { orchestrator.loadSessions(); taskStore.refresh(projects.map((p) => p.path)); }}
        onSelectLoop={(id) => { loopStore.setActiveLoopId(id); }}
        onTickLoop={(id) => { loopsApi.tick(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        onStopLoop={(id) => { loopsApi.stop(id).then(() => loopStore.refreshAllLoops(projects.map(p => p.id))); }}
        selectedLoopId={activeLoopId}
      />
  {/if}

  <section class="flex-1 flex flex-col relative bg-main overflow-hidden">
    <div class="flex-1 relative p-4 pr-0 overflow-hidden">
    {#if showProjectForm}
      <div class="absolute inset-0 flex items-center justify-center bg-scrim z-10">
        <ProjectForm onCreated={() => { showProjectForm = false; projectStore.loadProjects(); }} onCancel={() => (showProjectForm = false)} />
      </div>
    {/if}

    {#if showSessionForm}
    <FormDialog title="New Session" onClose={() => { showSessionForm = false; taskPrefill = null; tick().then(() => refocusTerminal()); }}>
      <SessionForm
        {projects}
        {sessions}
        {taskPrefill}
        currentProjectId={taskPrefill?.projectId ?? sessions.find(s => s.id === activeSessionId)?.project_id ?? null}
        onCreated={(session) => { showSessionForm = false; orchestrator.createSession(session); focusTerminal(); }}
        onCancel={() => { showSessionForm = false; taskPrefill = null; tick().then(() => refocusTerminal()); }}
      />
    </FormDialog>
    {/if}

    {#if showTaskForm}
    <FormDialog title="New Task" onClose={() => { showTaskForm = false; tick().then(() => refocusTerminal()); }}>
      <TaskForm
        mode="create"
        {projects}
        tasks={taskStore.getAllTasks()}
        onSubmitted={() => { showTaskForm = false; taskStore.refresh(projects.map((p) => p.path)); focusTerminal(); }}
        onCancel={() => { showTaskForm = false; tick().then(() => refocusTerminal()); }}
      />
    </FormDialog>
    {/if}

    {#if showLoopForm}
    <FormDialog title="Start Loop" onClose={() => { showLoopForm = false; tick().then(() => refocusTerminal()); }}>
      <LoopForm
        projectId={projects[0]?.id ?? ""}
        projectPath={projects[0]?.path ?? ""}
        onCreated={(loop) => { showLoopForm = false; loopStore.setActiveLoopId(loop.id); loopStore.refreshAllLoops(projects.map(p => p.id)); focusTerminal(); }}
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
      onSelectSession={(id) => { orchestrator.selectSession(id); focusTerminal(); }}
      onArchiveSession={() => { if (activeSessionId) { const s = sessions.find(x => x.id === activeSessionId); if (s) orchestrator.archiveSession(s); } }}
      onDeleteSession={() => { if (activeSessionId) { const s = sessions.find(x => x.id === activeSessionId); if (s) sessionToDelete = s; } }}
      onRenameSession={() => { if (activeSessionId) { sidebarVisible = true; renamingSessionId = activeSessionId; } }}
      onRestoreSession={async (id) => { await sessionsApi.restore(id); await orchestrator.loadSessions(); }}
      onDestroyArchivedSession={async (id) => { await sessionsApi.destroy(id); }}
      onNewSession={() => { if (projects.length === 0) showProjectForm = true; else showSessionForm = true; }}
      onResetTerminal={() => { if (activeSessionId) pty.write(activeSessionId, [0x0c]); }}
      onArchiveProject={async (id) => { await projectStore.archiveProject(id); }}
      onDeleteProject={(id) => { const p = projects.find(x => x.id === id); if (p) projectToDelete = p; }}
      onRestoreProject={async (id) => { await projectStore.restoreProject(id); }}
      onPickTask={(task) => { taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: `${task.key}: ${task.title}`, prompt: "" }; showSessionForm = true; }}
      onCreateTask={() => { showTaskForm = true; }}
      onToggleDiff={() => orchestrator.toggleDiff()}
      onOpenFile={(path) => orchestrator.openFile(path)}
      onOpenLogViewer={logViewerEnabled ? () => { showLogViewer = true; } : undefined}
      onCreatePr={openPrForm}
    />

    <KeyboardShortcuts open={showShortcuts} onOpenChange={(v) => (showShortcuts = v)} />

    {#each sessions as session (session.id)}
      {@const tabs = getTabs(session.id)}
      {@const activeTab = getActiveTabIndex(session.id)}
      {@const hasDiff = diffTabOpen[session.id] ?? false}
      {@const isDiffActive = diffTabActive[session.id] ?? false}
      {@const hasEditor = editorTabOpen[session.id] ?? false}
      {@const isEditorActive = editorTabActive[session.id] ?? false}
      {@const project = projects.find((p) => p.id === session.project_id)}
      {#each tabs as tab (tab.index)}
        {@const ptyKey = tab.index === 0 ? session.id : `${session.id}:${tab.index}`}
        {#if poolIsMounted(session.id)}
        <Terminal
          sessionId={ptyKey}
          visible={session.id === activeSessionId && tab.index === activeTab && !isDiffActive && !isEditorActive && !activeLoopId}
          focused={session.id === activeSessionId && tab.index === activeTab && !isDiffActive && !isEditorActive && !activeLoopId && zone === "terminal" && !showNewItemModal && !sessionToDelete && !showTaskForm && !showPrPanel}
          exited={tab.index === 0 && session.status === "exited"}
          skipAttach={tab.index !== 0}
          onAttached={() => { if (tab.index === 0 && session.status === "exited") orchestrator.updateSessionStatus(session.id, "active"); }}
          onUserInput={() => { if (agentStates[session.id]) orchestrator.clearAgentState(session.id); orchestrator.clearReviewReady(session.id); }}
        />
        {/if}
      {/each}
      {#if hasDiff && project}
        {@const repoPath = session.worktree_path ?? project.path}
        {@const baseBranch = session.base_branch ?? "main"}
        <ReviewTab
          {repoPath}
          {baseBranch}
          visible={session.id === activeSessionId && isDiffActive}
          sessionId={session.id}
          onEditFile={(filePath) => orchestrator.openFile(filePath)}
          onFileChange={(name) => orchestrator.setDiffFileName(session.id, name)}
        />
      {/if}
      {#if hasEditor && project}
        {@const editorRepoPath = session.worktree_path ?? project.path}
        <EditorTab
          repoPath={editorRepoPath}
          visible={session.id === activeSessionId && isEditorActive}
          theme={isDark() ? "vs-dark" : "vs"}
          onClose={() => orchestrator.closeEditorTab(session.id)}
          onFocusEditor={() => orchestrator.focusEditorTab(session.id)}
          onFileChange={(name) => orchestrator.setEditorFileName(session.id, name)}
          onModifiedChange={(mod) => orchestrator.setEditorModified(session.id, mod)}
          bind:this={editorBindRefs[session.id]}
        />
      {/if}
    {/each}

    {#if activeLoopId}
      <div class="w-full h-full bg-main">
        <LoopDashboard
          loopId={activeLoopId}
          onSelectSession={(sessionId) => { loopStore.setActiveLoopId(null); orchestrator.selectSession(sessionId); }}
          onOpenArtifact={(path) => { if (activeSession) orchestrator.openFile(path); }}
        />
      </div>
    {/if}

    {#if sessions.length === 0 && !showProjectForm && !showSessionForm && !activeLoopId}
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
            <button class="flex items-center gap-[11px] h-[40px] px-[11px] rounded-[9px] bg-accent-bg" onclick={() => { showNewItemModal = false; if (projects.length === 0) showProjectForm = true; else showSessionForm = true; }}>
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
              <span class="flex-1 text-[13.5px] text-t1">Loop</span>
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
        onOpenFile={(path) => orchestrator.openFile(path)}
        onPinFile={(path) => orchestrator.openFile(path)}
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

<PostMergePrompt />
<JiraDepartedPrompt />
