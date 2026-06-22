<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { listen } from "@tauri-apps/api/event";
  import { sessions as sessionsApi, pr as prApi, pty, notify, sessionLogs } from "./lib/api";
  import type { Session, Project } from "./lib/types";
  import { focusTerminal, focusExplorer, getActiveZone } from "./lib/focus.svelte";
  import * as projectStore from "./lib/project-store.svelte";
  import * as taskStore from "./lib/task-store.svelte";
  import { installKeyboardRouter, MOD_LABEL } from "./lib/keyboard";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import * as navCycle from "./lib/session-nav-cycle.svelte";
  import { computeSidebarSessionOrder } from "./lib/sidebar-session-order";
  import { loadSettings, getSettings, isDark } from "./lib/settings.svelte";
  import { loadTheme } from "./lib/theme-loader";
  import { startPolling as startCiPolling } from "./lib/ci-checks.svelte";
  import { getSnackbarMessage, getSnackbarType, dismissSnackbar, showSnackbar } from "./lib/snackbar.svelte";
  import { Dialog } from "bits-ui";
  import Titlebar from "./components/Titlebar.svelte";
  import UnifiedSidebar from "./components/UnifiedSidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import Terminal from "./components/Terminal.svelte";
  import TabSwitcher from "./components/TabSwitcher.svelte";
  import CommandMenu from "./components/CommandMenu.svelte";
  import ReviewTab from "./components/ReviewTab.svelte";
  import EditorTab from "./components/EditorTab.svelte";
  import FileExplorer from "./components/FileExplorer.svelte";
  import KeyboardShortcuts from "./components/KeyboardShortcuts.svelte";
  import LogViewer from "./components/LogViewer.svelte";
  import { getTabs, getActiveTabIndex } from "./lib/session-tabs.svelte";
  import { isMounted as poolIsMounted, isPaused as poolIsPaused } from "./lib/terminal-pool.svelte";
  import * as orchestrator from "./lib/session-orchestrator.svelte";

  // ─── UI-only state ──────────────────────────────────────────────────────────
  let showProjectForm = $state(false);
  let showSessionForm = $state(false);
  let sidebarVisible = $state(true);
  let taskCreateRequested = $state(false);
  let commandMenuOpen = $state(false);
  let commandMenuFileMode = $state(false);
  let showNewItemModal = $state(false);
  let showShortcuts = $state(false);
  let showHookPrompt = $state(false);
  let showQuitConfirm = $state(false);
  let quitDirectCount = $state(0);
  let fileExplorerVisible = $state(false);
  let showLogViewer = $state(false);

  // PR form state
  let showPrForm = $state(false);
  let prTitle = $state("");
  let prBody = $state("");
  let prBaseBranch = $state("");
  let prDraft = $state(false);
  let prSubmitting = $state(false);
  let prError = $state("");

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
  let taskPrefill = $state<{ key: string; title: string; description: string; branch: string; name: string; prompt: string; projectId?: string | null } | null>(null);

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
  const activeProjectName = $derived(activeSession ? (projects.find((p) => p.id === activeSession.project_id)?.name ?? null) : null);
  const activeSessionName = $derived(activeSession ? (activeSession.name || activeSession.branch) : null);

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
    new WebviewWindow("preferences", { url: "index.html?page=preferences", title: "Preferences", width: 700, height: 550, parent: getCurrentWindow(), resizable: true, minimizable: false, maximizable: false });
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
    });
    orchestrator.loadSessions();
    loadSettings().then(() => loadTheme());

    const cleanupEvents = orchestrator.startEventListeners();
    const cleanupSymphony = orchestrator.startSymphonyPolling();
    const cleanupCi = startCiPolling(orchestrator.getSessions());
    const unlistenSettings = listen("settings-changed", () => { loadSettings().then(() => loadTheme()); });
    const unlistenCleanup = listen<string>("cleanup-error", (event) => { showSnackbar(event.payload); });

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
          if (!sw.isCycling) startCycle(activeSessionId ?? undefined, new Set(sessions.map((s) => s.id)));
          else advance(1);
        } else if (action.type === "tab_switch_reverse") {
          const sw = getCycleState();
          if (!sw.isCycling) { startCycle(activeSessionId ?? undefined, new Set(sessions.map((s) => s.id))); advance(-1); }
          else advance(-1);
        } else if (action.type === "focus_terminal") {
          if (getCycleState().isCycling) cancel();
          if (navCycle.isCycling()) navCycle.cancel();
          showSessionForm = false; showProjectForm = false; showShortcuts = false; showNewItemModal = false; sessionToDelete = null; commandMenuOpen = false; commandMenuFileMode = false;
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
      },
      () => !showSessionForm && !showProjectForm && !commandMenuOpen && !showShortcuts && !showNewItemModal && !getCycleState().isCycling && !navCycle.isCycling(),
      () => !!(activeSessionId && editorTabActive[activeSessionId])
    );

    function onKeyUp(e: KeyboardEvent) {
      const isModRelease = (e.key === "Control" && !e.ctrlKey) || (e.key === "Meta" && !e.metaKey);
      if (!isModRelease) return;
      if (getCycleState().isCycling) { const target = commit(); if (target) orchestrator.selectSession(target); focusTerminal(); }
      if (navCycle.isCycling()) { const target = navCycle.commit(); if (target) orchestrator.selectSession(target); focusTerminal(); }
    }
    function onBlur() { setTimeout(() => { if (!document.hasFocus()) { if (getCycleState().isCycling) cancel(); if (navCycle.isCycling()) navCycle.cancel(); } }, 0); }
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);

    return () => { cleanup(); cleanupEvents(); cleanupSymphony(); cleanupCi(); unlistenSettings.then((fn) => fn()); unlistenCleanup.then((fn) => fn()); unlistenClose.then((fn) => fn()); window.removeEventListener("keyup", onKeyUp); window.removeEventListener("blur", onBlur); };
  });
</script>

<main class="flex flex-col h-screen">
  <Titlebar
    projectName={activeProjectName}
    sessionName={activeSessionName}
    {sidebarVisible}
    prUrl={sessions.find(s => s.id === activeSessionId)?.pr_url ?? null}
    prState={sessions.find(s => s.id === activeSessionId)?.pr_state ?? null}
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
    {symphonyStatus}
  />

  <div class="flex flex-1 min-h-0">
  {#if sidebarVisible}
      <UnifiedSidebar
        {renamingSessionId}
        {taskCreateRequested}
        onSelectSession={(id) => orchestrator.selectSession(id)}
        onArchiveSession={(s) => orchestrator.archiveSession(s)}
        onDeleteSession={(s) => (sessionToDelete = s)}
        onRestartSession={(s) => orchestrator.restartSession(s)}
        onRenameSession={doRename}
        onStartRename={(id) => { renamingSessionId = id || null; if (!id) focusTerminal(); }}
        onDeleteProject={(p) => (projectToDelete = p)}
        onPickTask={(task, repoPath) => { const proj = projects.find(p => p.path === repoPath); taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: `${task.key}: ${task.title}`, prompt: "", projectId: proj?.id ?? null }; showSessionForm = true; }}
        onAddProject={() => (showProjectForm = true)}
        onOpenPreferences={openPreferences}
        onCreateSession={() => { showNewItemModal = true; }}
        onTaskCreateConsumed={() => { taskCreateRequested = false; }}
        onSessionsChanged={() => { orchestrator.loadSessions(); taskStore.refresh(projects.map((p) => p.path)); }}
      />
  {/if}

  <section class="flex-1 flex flex-col relative bg-surface-50 dark:bg-surface-950 overflow-hidden">
    <div class="flex-1 relative p-4 pr-0 overflow-hidden">
    {#if showProjectForm}
      <div class="absolute inset-0 flex items-center justify-center bg-black/50 z-10">
        <ProjectForm onCreated={() => { showProjectForm = false; projectStore.loadProjects(); }} onCancel={() => (showProjectForm = false)} />
      </div>
    {/if}

    <Dialog.Root bind:open={showSessionForm}>
      <Dialog.Portal>
        <Dialog.Overlay class="fixed inset-0 z-40 bg-black/55 dark:bg-[rgba(25,25,30,0.55)]" />
        <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-[452px] -translate-x-1/2 -translate-y-1/2 rounded-xl border border-border-strong bg-surface-50 dark:bg-surface-700 shadow-[0_26px_70px_-16px_rgba(0,0,0,0.5)] overflow-hidden">
          <Dialog.Title class="px-5 pt-5 pb-3 text-[13px] font-semibold text-text-1">New Session</Dialog.Title>
          <SessionForm
            {projects}
            {sessions}
            {taskPrefill}
            currentProjectId={taskPrefill?.projectId ?? sessions.find(s => s.id === activeSessionId)?.project_id ?? null}
            onCreated={(session) => { showSessionForm = false; orchestrator.createSession(session); focusTerminal(); }}
            onCancel={() => { showSessionForm = false; taskPrefill = null; }}
          />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>

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
      onCreateTask={() => { if (!sidebarVisible) sidebarVisible = true; requestAnimationFrame(() => { taskCreateRequested = true; }); }}
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
          visible={session.id === activeSessionId && tab.index === activeTab && !isDiffActive && !isEditorActive}
          focused={session.id === activeSessionId && tab.index === activeTab && !isDiffActive && !isEditorActive && zone === "terminal"}
          exited={tab.index === 0 && session.status === "exited"}
          skipAttach={tab.index !== 0}
          paused={poolIsPaused(session.id)}
          onAttached={() => { if (tab.index === 0 && session.status === "exited") orchestrator.updateSessionStatus(session.id, "active"); }}
          onUserInput={() => { if (agentStates[session.id]) orchestrator.clearAgentState(session.id); }}
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

    {#if sessions.length === 0 && !showProjectForm && !showSessionForm}
      <div class="flex items-center justify-center h-full">
        <p class="text-surface-700 dark:text-surface-300">No active session. Press <kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">{MOD_LABEL}N</kbd> to create one.</p>
      </div>
    {/if}

    {#if showHookPrompt}
      <div class="absolute top-2 left-4 right-4 z-20 flex items-center gap-3 rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-950 px-4 py-2.5 shadow-sm">
        <span class="text-sm text-amber-800 dark:text-amber-200">Install notification hook for instant agent-done alerts?</span>
        <button class="ml-auto rounded bg-amber-600 px-3 py-1 text-xs font-medium text-white hover:bg-amber-700" onclick={async () => { await notify.install(); showHookPrompt = false; }}>Install</button>
        <button class="rounded px-2 py-1 text-xs text-surface-600 dark:text-surface-400 hover:text-surface-800 dark:hover:text-surface-200" onclick={() => (showHookPrompt = false)}>Dismiss</button>
      </div>
    {/if}

    {#if sessionToDelete}
      <Dialog.Root open={true} onOpenChange={(v) => { if (!v) sessionToDelete = null; }}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-50" />
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none" onkeydown={(e) => { if (e.key === 'c' || e.key === 'n') sessionToDelete = null; if (e.key === 'd' || e.key === 'y') { const s = sessionToDelete; sessionToDelete = null; if (s) orchestrator.deleteSession(s); } }}>
            <Dialog.Title class="text-sm">Delete session <strong>{sessionToDelete.name || sessionToDelete.branch}</strong>?</Dialog.Title>
            <div class="flex justify-between">
              <span class="text-sm text-surface-500 dark:text-surface-400"><kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">n</kbd>/<kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">c</kbd> cancel</span>
              <span class="text-sm text-surface-500 dark:text-surface-400"><kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">d</kbd>/<kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">y</kbd> delete</span>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}
    {#if projectToDelete}
      {@const ptd = projectToDelete}
      {@const projSessions = sessions.filter((s) => s.project_id === ptd.id)}
      {@const worktreeCount = projSessions.filter((s) => s.worktree_path).length}
      <Dialog.Root open={true} onOpenChange={(v) => { if (!v) projectToDelete = null; }}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-50" />
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none" onkeydown={(e) => { if (e.key === 'c' || e.key === 'n') projectToDelete = null; if (e.key === 'd' || e.key === 'y') deleteProject(ptd); }}>
            <Dialog.Title class="text-sm">Delete project <strong>{ptd.name}</strong>?</Dialog.Title>
            <p class="text-xs text-surface-500 dark:text-surface-400">This will permanently remove {projSessions.length} session{projSessions.length !== 1 ? 's' : ''}{#if worktreeCount > 0} and clean up {worktreeCount} worktree{worktreeCount !== 1 ? 's' : ''}{/if}. This cannot be undone.</p>
            <div class="flex justify-between">
              <span class="text-sm text-surface-500 dark:text-surface-400"><kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">n</kbd>/<kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">c</kbd> cancel</span>
              <span class="text-sm text-surface-500 dark:text-surface-400"><kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">d</kbd>/<kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">y</kbd> delete</span>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}
    {#if showQuitConfirm}
      <Dialog.Root open={true} onOpenChange={(v) => { if (!v) showQuitConfirm = false; }}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-50" />
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none" onkeydown={(e) => { if (e.key === 'Escape' || e.key === 'n') showQuitConfirm = false; if (e.key === 'q' || e.key === 'y') { showQuitConfirm = false; getCurrentWindow().destroy(); } }}>
            <Dialog.Title class="text-sm font-medium">{quitDirectCount} active session{quitDirectCount > 1 ? 's' : ''} will be terminated.</Dialog.Title>
            <p class="text-xs text-surface-500 dark:text-surface-400">Direct sessions don't survive app quit.</p>
            <div class="flex justify-between">
              <span class="text-sm text-surface-500 dark:text-surface-400"><kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">n</kbd> cancel</span>
              <span class="text-sm text-surface-500 dark:text-surface-400"><kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">q</kbd> quit</span>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}

    {#if showNewItemModal}
      <Dialog.Root open={true} onOpenChange={(v) => { if (!v) showNewItemModal = false; }}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-50" />
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-64 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-5 space-y-3 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none" onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Escape') showNewItemModal = false; if (e.key === 's') { showNewItemModal = false; if (projects.length === 0) showProjectForm = true; else showSessionForm = true; } if (e.key === 't') { showNewItemModal = false; taskCreateRequested = true; } }}>
            <Dialog.Title class="text-sm font-semibold text-surface-900 dark:text-surface-50">New…</Dialog.Title>
            <div class="space-y-1">
              <button class="w-full text-left px-3 py-2 rounded-md text-sm text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800 flex items-center justify-between" onclick={() => { showNewItemModal = false; if (projects.length === 0) showProjectForm = true; else showSessionForm = true; }}>
                Session
                <kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs text-surface-500">s</kbd>
              </button>
              <button class="w-full text-left px-3 py-2 rounded-md text-sm text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800 flex items-center justify-between" onclick={() => { showNewItemModal = false; taskCreateRequested = true; }}>
                Task
                <kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs text-surface-500">t</kbd>
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}

    {#if showLogViewer}
      <div class="absolute inset-0 z-30">
        <LogViewer onClose={() => { showLogViewer = false; }} />
      </div>
    {/if}
    </div>
    <!-- Keyboard helper bar -->
    <div class="h-[34px] shrink-0 flex items-center gap-4 px-4 bg-surface-100 dark:bg-surface-800 border-t border-border text-[11px] text-text-3 select-none">
      <span><kbd class="font-mono text-[10px] bg-surface-200 dark:bg-surface-600 px-1 rounded">⌘K</kbd> Command</span>
      <span><kbd class="font-mono text-[10px] bg-surface-200 dark:bg-surface-600 px-1 rounded">⌘N</kbd> New</span>
      <span><kbd class="font-mono text-[10px] bg-surface-200 dark:bg-surface-600 px-1 rounded">⌘B</kbd> Sidebar</span>
      <span><kbd class="font-mono text-[10px] bg-surface-200 dark:bg-surface-600 px-1 rounded">⌃⇥</kbd> Switch</span>
      <span><kbd class="font-mono text-[10px] bg-surface-200 dark:bg-surface-600 px-1 rounded">⌘1–9</kbd> Jump</span>
      <span><kbd class="font-mono text-[10px] bg-surface-200 dark:bg-surface-600 px-1 rounded">⌘\</kbd> Diff</span>
    </div>
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
  <Dialog.Root open={true} onOpenChange={(v) => { if (!v) showPrForm = false; }}>
    <Dialog.Portal>
      <Dialog.Overlay class="fixed inset-0 z-50" />
      <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-[36rem] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-5 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none">
        <Dialog.Title class="text-sm font-medium text-surface-900 dark:text-surface-50 mb-4">Create Pull Request</Dialog.Title>
        <div class="flex flex-col gap-3">
          <div>
            <label for="pr-title" class="text-xs font-medium text-surface-600 dark:text-surface-400 mb-1 block">Title</label>
            <input id="pr-title" type="text" bind:value={prTitle} class="w-full px-2 py-1.5 text-sm rounded border border-surface-300 dark:border-surface-600 bg-white dark:bg-surface-800 text-surface-900 dark:text-surface-100 focus:outline-none focus:ring-1 focus:ring-primary-500" />
          </div>
          <div>
            <label for="pr-body" class="text-xs font-medium text-surface-600 dark:text-surface-400 mb-1 block">Body</label>
            <textarea id="pr-body" bind:value={prBody} rows="10" class="w-full px-2 py-1.5 text-sm rounded border border-surface-300 dark:border-surface-600 bg-white dark:bg-surface-800 text-surface-900 dark:text-surface-100 resize-y focus:outline-none focus:ring-1 focus:ring-primary-500 font-mono text-xs"></textarea>
          </div>
          <div>
            <label for="pr-base" class="text-xs font-medium text-surface-600 dark:text-surface-400 mb-1 block">Base branch</label>
            <input id="pr-base" type="text" bind:value={prBaseBranch} class="w-full px-2 py-1.5 text-sm rounded border border-surface-300 dark:border-surface-600 bg-white dark:bg-surface-800 text-surface-900 dark:text-surface-100 focus:outline-none focus:ring-1 focus:ring-primary-500" />
          </div>
          <label class="flex items-center gap-2 text-xs text-surface-700 dark:text-surface-300">
            <input type="checkbox" bind:checked={prDraft} class="rounded border-surface-300 dark:border-surface-600" />
            Draft PR
          </label>
          {#if prError}
            <p class="text-xs text-error-500">{prError}</p>
          {/if}
          <div class="flex justify-end gap-2 mt-1">
            <button class="px-2 py-1 text-xs rounded border border-surface-300 text-surface-700 hover:bg-surface-100 dark:border-surface-600 dark:text-surface-300 dark:hover:bg-surface-800" onclick={() => (showPrForm = false)}>Cancel</button>
            <button class="px-2 py-1 text-xs rounded bg-surface-900 text-surface-50 hover:bg-surface-800 dark:bg-surface-50 dark:text-surface-900 dark:hover:bg-surface-200 disabled:opacity-50" disabled={prSubmitting || !prTitle.trim()} onclick={submitPr}>
              {prSubmitting ? "Creating…" : "Create"}
            </button>
          </div>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

{#if getSnackbarMessage()}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed bottom-4 left-4 z-[100] max-w-lg cursor-pointer rounded-lg {getSnackbarType() === 'error' ? 'bg-red-600' : 'bg-green-600'} px-4 py-3 shadow-lg" onclick={() => { navigator.clipboard.writeText(getSnackbarMessage()!); dismissSnackbar(); }} title="Click to copy and dismiss">
    <p class="text-sm text-white font-mono break-all">{getSnackbarMessage()}</p>
    <p class="text-xs {getSnackbarType() === 'error' ? 'text-red-200' : 'text-green-200'} mt-1">Click to dismiss</p>
  </div>
{/if}
