<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { focusTerminal, getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import { touchMru, removeMru, getMruList } from "./lib/mru.svelte";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import { loadSettings, getSettings, isDark } from "./lib/settings.svelte";
  import { loadTheme, extractTerminalTheme } from "./lib/theme-loader";
  import { getSnackbarMessage, dismissSnackbar, showSnackbar } from "./lib/snackbar.svelte";
  import { playTaskComplete } from "./lib/soundPlayer";
  import { Dialog } from "bits-ui";
  import Titlebar from "./components/Titlebar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import Terminal from "./components/Terminal.svelte";
  import TabSwitcher from "./components/TabSwitcher.svelte";
  import CommandMenu from "./components/CommandMenu.svelte";
  import TabBar from "./components/TabBar.svelte";
  import DiffTab from "./components/DiffTab.svelte";
  import KeyboardShortcuts from "./components/KeyboardShortcuts.svelte";
  import { initSession, getTabs, addTab, removeTab, setActiveTab, getActiveTabIndex, getTabCount, destroySession as destroyTabState } from "./lib/session-tabs.svelte";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Session {
    id: string;
    project_id: string;
    name: string;
    tmux_name: string | null;
    branch: string;
    status: string;
    created_at: string;
    worktree_path: string | null;
    backend: string;
    tab_count: number;
    base_branch: string | null;
    task_key: string | null;
  }

  let projects = $state<Project[]>([]);
  let sessions = $state<Session[]>([]);
  let activeSessionId = $state<string | null>(null);
  let showProjectForm = $state(false);
  let sidebarVisible = $state(true);

  let showSessionForm = $state(false);
  let taskPrefill = $state<{ key: string; title: string; description: string; branch: string; name: string; prompt: string } | null>(null);

  // Command menu state
  let commandMenuOpen = $state(false);

  // Agent state tracking (Busy/Idle per session)
  let agentStates = $state<Record<string, string>>({});

  // Preferences state
  let showShortcuts = $state(false);

  async function openPreferences() {
    const existing = await WebviewWindow.getByLabel("preferences");
    if (existing) {
      existing.setFocus();
      return;
    }
    new WebviewWindow("preferences", {
      url: "index.html?page=preferences",
      title: "Preferences",
      width: 700,
      height: 550,
      parent: getCurrentWindow(),
      resizable: true,
      minimizable: false,
      maximizable: false,
    });
  }

  // Hook install prompt
  let showHookPrompt = $state(false);

  // Quit confirmation
  let showQuitConfirm = $state(false);
  let quitDirectCount = $state(0);

  // Diff tab state: set of session IDs that have diff tab open
  let diffTabOpen = $state<Record<string, boolean>>({});
  let diffTabActive = $state<Record<string, boolean>>({});

  // Delete confirmation state
  let sessionToDelete = $state<Session | null>(null);
  let projectToDelete = $state<Project | null>(null);

  // Rename state
  let renamingSessionId = $state<string | null>(null);

  async function doRename(id: string, name: string) {
    await invoke("rename_session", { id, name });
    sessions = sessions.map((s) => s.id === id ? { ...s, name } : s);
    renamingSessionId = null;
    focusTerminal();
  }

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  async function loadSessions() {
    sessions = await invoke<Session[]>("list_sessions");
    // Initialize tab state for each session
    for (const s of sessions) {
      if (getTabCount(s.id) === 0) {
        initSession(s.id, s.tab_count);
        // Helper tabs are spawned by the Terminal component when it mounts
        if (s.status === "active") {
          for (let i = 1; i < s.tab_count; i++) {
            listenForTabExit(s.id, i);
          }
        }
      }
    }
    listenForExits();
    // On initial load, activate the first session (reconnection)
    if (sessions.length > 0 && !activeSessionId) {
      // Seed MRU with all sessions (active one first)
      for (let i = sessions.length - 1; i >= 0; i--) {
        touchMru(sessions[i].id);
      }
      selectSession(sessions[0].id);
    }
  }

  // Listen for pty-exited events (session process terminated)
  let exitUnlisteners: Array<() => void> = [];
  function listenForExits() {
    exitUnlisteners.forEach((fn) => fn());
    exitUnlisteners = [];
    for (const s of sessions) {
      if (s.status === "active") {
        listen(`pty-exited-${s.id}`, () => {
          // Skip if session was already removed (e.g., deleted by user)
          if (!sessions.find((x) => x.id === s.id)) return;
          sessions = sessions.map((x) => x.id === s.id ? { ...x, status: "exited" } : x);
          invoke("mark_exited", { sessionId: s.id });
        }).then((unlisten) => exitUnlisteners.push(unlisten));
      }
    }
  }

  function selectSession(id: string) {
    activeSessionId = id;
    touchMru(id);
    // Clear idle state when user focuses this session
    if (agentStates[id] === "Idle") {
      agentStates = { ...agentStates, [id]: "Busy" };
    }
    // Tell backend to allow future notifications for this session
    invoke("acknowledge_session", { sessionId: id });
  }

  function jumpToSession(index: number) {
    if (index < sessions.length) {
      selectSession(sessions[index].id);
    }
  }

  async function handleNewTab() {
    if (!activeSessionId) return;
    const tabIndex = addTab(activeSessionId);
    if (tabIndex === -1) return;
    setActiveTab(activeSessionId, tabIndex);
    listenForTabExit(activeSessionId, tabIndex);
  }

  async function handleCloseTab() {
    if (!activeSessionId) return;
    const active = getActiveTabIndex(activeSessionId);
    if (active === 0) {
      // No helper tab active — close window (original Cmd+W behavior)
      getCurrentWindow().close();
      return;
    }
    removeTab(activeSessionId, active);
    await invoke("close_tab", { sessionId: activeSessionId, tabIndex: active });
  }

  function handleNextTab() {
    if (!activeSessionId) return;
    const tabs = getTabs(activeSessionId);
    if (tabs.length <= 1) return;
    const active = getActiveTabIndex(activeSessionId);
    const currentPos = tabs.findIndex((t) => t.index === active);
    const nextPos = (currentPos + 1) % tabs.length;
    setActiveTab(activeSessionId, tabs[nextPos].index);
  }

  function handlePrevTab() {
    if (!activeSessionId) return;
    const tabs = getTabs(activeSessionId);
    if (tabs.length <= 1) return;
    const active = getActiveTabIndex(activeSessionId);
    const currentPos = tabs.findIndex((t) => t.index === active);
    const prevPos = (currentPos - 1 + tabs.length) % tabs.length;
    setActiveTab(activeSessionId, tabs[prevPos].index);
  }

  function handleToggleDiff() {
    if (!activeSessionId) return;
    if (diffTabOpen[activeSessionId]) {
      // If diff is active, close it. If terminal is active, close diff.
      if (diffTabActive[activeSessionId]) {
        diffTabActive = { ...diffTabActive, [activeSessionId]: false };
        diffTabOpen = { ...diffTabOpen, [activeSessionId]: false };
      } else {
        // Switch to diff tab
        diffTabActive = { ...diffTabActive, [activeSessionId]: true };
      }
    } else {
      // Open diff tab and make it active
      diffTabOpen = { ...diffTabOpen, [activeSessionId]: true };
      diffTabActive = { ...diffTabActive, [activeSessionId]: true };
    }
  }

  function listenForTabExit(sessionId: string, tabIndex: number) {
    const ptyKey = `${sessionId}:${tabIndex}`;
    listen(`pty-exited-${ptyKey}`, () => {
      removeTab(sessionId, tabIndex);
      invoke("close_tab", { sessionId, tabIndex });
    }).then((unlisten) => exitUnlisteners.push(unlisten));
  }

  onMount(() => {
    loadProjects();
    loadSessions();
    loadSettings().then(() => loadTheme());

    // Reload settings/theme when changed from preferences window
    const unlistenSettings = listen("settings-changed", () => {
      loadSettings().then(() => loadTheme());
    });

    // Check if notification hook is installed
    invoke<boolean>("is_notify_hook_installed").then((installed) => {
      if (!installed) showHookPrompt = true;
    });

    // Quit confirmation for active direct sessions
    const unlistenClose = getCurrentWindow().onCloseRequested(async (event) => {
      const activeDirectCount = sessions.filter(
        (s) => s.status === "active" && s.backend === "direct"
      ).length;
      if (activeDirectCount > 0) {
        event.preventDefault();
        quitDirectCount = activeDirectCount;
        showQuitConfirm = true;
      }
    });

    // Listen for agent state changes from backend
    const unlistenState = listen<{ session_id: string; state: string }>("agent-state-change", (event) => {
      console.log("[notify] agent-state-change:", event.payload.session_id, "→", event.payload.state);
      agentStates = { ...agentStates, [event.payload.session_id]: event.payload.state };
      if (event.payload.state === "Idle") {
        console.log("[notify] playing task complete sound");
        playTaskComplete();
        invoke("fire_task_notify_hook", { sessionId: event.payload.session_id }).catch(() => {});
      }
    });

    const cleanup = installKeyboardRouter(
      (action) => {
      if (action.type === "new_session") {
        if (projects.length === 0) {
          showProjectForm = true;
        } else {
          showSessionForm = true;
        }
      } else if (action.type === "new_project") {
        showProjectForm = true;
      } else if (action.type === "toggle_sidebar") {
        sidebarVisible = !sidebarVisible;
      } else if (action.type === "jump_to_session") {
        jumpToSession(action.index);
      } else if (action.type === "tab_switch") {
        const switcher = getCycleState();
        if (!switcher.isCycling) {
          startCycle(activeSessionId ?? undefined);
        } else {
          advance(1);
        }
      } else if (action.type === "tab_switch_reverse") {
        const switcher = getCycleState();
        if (!switcher.isCycling) {
          startCycle(activeSessionId ?? undefined);
          // After startCycle, index is 0 (next MRU). For reverse, go to end.
          advance(-1);
        } else {
          advance(-1);
        }
      } else if (action.type === "focus_terminal") {
        const switcher = getCycleState();
        if (switcher.isCycling) {
          cancel();
        }
        showSessionForm = false;
        showProjectForm = false;
        showShortcuts = false;
        sessionToDelete = null;
        commandMenuOpen = false;
      } else if (action.type === "command_palette") {
        commandMenuOpen = !commandMenuOpen;
      } else if (action.type === "open_preferences") {
        openPreferences();
      } else if (action.type === "show_shortcuts") {
        showShortcuts = !showShortcuts;
      } else if (action.type === "new_tab") {
        handleNewTab();
      } else if (action.type === "close_tab") {
        handleCloseTab();
      } else if (action.type === "next_tab") {
        handleNextTab();
      } else if (action.type === "prev_tab") {
        handlePrevTab();
      } else if (action.type === "toggle_diff") {
        handleToggleDiff();
      }
    },
    () => !showSessionForm && !showProjectForm && !commandMenuOpen && !showShortcuts && !getCycleState().isCycling
    );

    // Listen for Ctrl release to commit tab switch
    function onKeyUp(e: KeyboardEvent) {
      const switcher = getCycleState();
      if (e.key === "Control" && switcher.isCycling) {
        const target = commit();
        if (target) selectSession(target);
        focusTerminal();
      }
    }

    function onBlur() {
      const switcher = getCycleState();
      if (switcher.isCycling) cancel();
    }

    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);

    return () => {
      cleanup();
      unlistenState.then((fn) => fn());
      unlistenSettings.then((fn) => fn());
      unlistenClose.then((fn) => fn());
      exitUnlisteners.forEach((fn) => fn());
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  });

  function onSessionCreated(session: Session) {
    showSessionForm = false;
    sessions = [...sessions, session];
    initSession(session.id, 1);
    selectSession(session.id);
    focusTerminal();
    listenForExits();
  }

  async function doDelete(s: Session) {
    await invoke("destroy_session", { id: s.id });
    destroyTabState(s.id);
    const { [s.id]: _d1, ...restOpen } = diffTabOpen;
    const { [s.id]: _d2, ...restActive } = diffTabActive;
    diffTabOpen = restOpen;
    diffTabActive = restActive;
    sessions = sessions.filter((x) => x.id !== s.id);
    removeMru(s.id);
    if (activeSessionId === s.id) {
      activeSessionId = sessions[0]?.id ?? null;
      if (activeSessionId) touchMru(activeSessionId);
    }
  }

  async function confirmDelete() {
    if (!sessionToDelete) return;
    const s = sessionToDelete;
    sessionToDelete = null;
    await doDelete(s);
  }

  async function archiveCurrentSession() {
    if (!activeSessionId) return;
    const s = sessions.find((x) => x.id === activeSessionId);
    if (!s) return;
    await archiveSession(s);
  }

  async function archiveSession(s: Session) {
    await invoke("archive_session", { id: s.id });
    sessions = sessions.filter((x) => x.id !== s.id);
    removeMru(s.id);
    if (activeSessionId === s.id) {
      activeSessionId = sessions[0]?.id ?? null;
      if (activeSessionId) touchMru(activeSessionId);
    }
  }

  async function archiveProject(p: Project) {
    await invoke("archive_project", { id: p.id });
    const projectSessionIds = sessions.filter((s) => s.project_id === p.id).map((s) => s.id);
    for (const id of projectSessionIds) removeMru(id);
    sessions = sessions.filter((s) => s.project_id !== p.id);
    projects = projects.filter((x) => x.id !== p.id);
    if (activeSessionId && projectSessionIds.includes(activeSessionId)) {
      activeSessionId = getMruList()[0] ?? null;
      if (activeSessionId) touchMru(activeSessionId);
    }
  }

  async function deleteProject(p: Project) {
    await invoke("delete_project", { id: p.id });
    const projectSessionIds = sessions.filter((s) => s.project_id === p.id).map((s) => s.id);
    for (const id of projectSessionIds) {
      removeMru(id);
      destroyTabState(id);
    }
    sessions = sessions.filter((s) => s.project_id !== p.id);
    projects = projects.filter((x) => x.id !== p.id);
    if (activeSessionId && projectSessionIds.includes(activeSessionId)) {
      activeSessionId = getMruList()[0] ?? null;
      if (activeSessionId) touchMru(activeSessionId);
    }
    projectToDelete = null;
  }

  async function restartSession(s: Session) {
    const updated = await invoke<Session>("restart_session", { sessionId: s.id });
    sessions = sessions.map((x) => x.id === s.id ? updated : x);
    selectSession(s.id);
    listenForExits();
  }

  function deleteCurrentSession() {
    if (!activeSessionId) return;
    const s = sessions.find((x) => x.id === activeSessionId);
    if (s) sessionToDelete = s;
  }

  const zone = $derived(getActiveZone());

  const activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);
  const activeProjectName = $derived(
    activeSession ? (projects.find((p) => p.id === activeSession.project_id)?.name ?? null) : null
  );
  const activeSessionName = $derived(
    activeSession ? (activeSession.name || activeSession.branch) : null
  );
</script>

<main class="flex flex-col h-screen">
  <Titlebar
    projectName={activeProjectName}
    sessionName={activeSessionName}
    {sidebarVisible}
    showDiffButton={!!activeSessionId}
    diffActive={!!(activeSessionId && diffTabActive[activeSessionId])}
    onToggleDiff={handleToggleDiff}
  />

  <div class="flex flex-1 min-h-0">
  {#if sidebarVisible}
    <Sidebar
      {projects}
      {sessions}
      {activeSessionId}
      {zone}
      {agentStates}
      {renamingSessionId}
      onAddProject={() => (showProjectForm = true)}
      onSelectSession={selectSession}
      onArchiveSession={(s) => archiveSession(s)}
      onDeleteSession={(s) => (sessionToDelete = s)}
      onRestartSession={restartSession}
      onOpenPreferences={openPreferences}
      onRenameSession={doRename}
      onStartRename={(id) => { renamingSessionId = id || null; if (!id) focusTerminal(); }}
      onArchiveProject={archiveProject}
      onDeleteProject={(p) => (projectToDelete = p)}
    />
  {/if}

  <section class="flex-1 relative p-4 pr-0 bg-surface-50 dark:bg-surface-950 overflow-hidden">
    {#if showProjectForm}
      <div class="absolute inset-0 flex items-center justify-center bg-black/50 z-10">
        <ProjectForm
          onCreated={() => { showProjectForm = false; loadProjects(); }}
          onCancel={() => (showProjectForm = false)}
        />
      </div>
    {/if}

    <Dialog.Root bind:open={showSessionForm}>
      <Dialog.Portal>
        <Dialog.Overlay class="fixed inset-0 z-40" />
        <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-96 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 shadow-lg dark:border-surface-700 dark:bg-surface-900">
          <Dialog.Title class="text-lg font-semibold mb-4">New Session</Dialog.Title>
          <SessionForm
            {projects}
            {sessions}
            {taskPrefill}
            currentProjectId={sessions.find(s => s.id === activeSessionId)?.project_id ?? null}
            onCreated={onSessionCreated}
            onCancel={() => { showSessionForm = false; taskPrefill = null; }}
          />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>

    {#if getCycleState().isVisible}
      <TabSwitcher
        mruSessionIds={getCycleState().cycleList}
        {sessions}
        {projects}
        selectedIndex={getCycleState().index}
        {agentStates}
      />
    {/if}

    <CommandMenu
      open={commandMenuOpen}
      {sessions}
      {projects}
      {activeSessionId}
      onOpenChange={(v) => (commandMenuOpen = v)}
      onSelectSession={(id) => { selectSession(id); focusTerminal(); }}
      onArchiveSession={archiveCurrentSession}
      onDeleteSession={deleteCurrentSession}
      onRenameSession={() => {
        if (!activeSessionId) return;
        sidebarVisible = true;
        const s = sessions.find((x) => x.id === activeSessionId);
        if (s) renamingSessionId = s.id;
      }}
      onRestoreSession={async (id) => {
        await invoke("restore_session", { id });
        await loadSessions();
      }}
      onDestroyArchivedSession={async (id) => {
        await invoke("destroy_session", { id });
      }}
      onNewSession={() => {
        if (projects.length === 0) {
          showProjectForm = true;
        } else {
          showSessionForm = true;
        }
      }}
      onResetTerminal={() => {
        if (activeSessionId) {
          invoke("write_to_pty", { sessionId: activeSessionId, data: [0x0c] });
        }
      }}
      onArchiveProject={async (id) => {
        const p = projects.find((x) => x.id === id);
        if (p) await archiveProject(p);
      }}
      onDeleteProject={(id) => {
        const p = projects.find((x) => x.id === id);
        if (p) projectToDelete = p;
      }}
      onRestoreProject={async (id) => {
        await invoke("restore_project", { id });
        await loadProjects();
      }}
      onPickTask={(task) => {
        taskPrefill = { key: task.key, title: task.title, description: task.description, branch: "", name: `${task.key}: ${task.title}`, prompt: "" };
        showSessionForm = true;
      }}
      onToggleDiff={handleToggleDiff}
    />

    <KeyboardShortcuts open={showShortcuts} onOpenChange={(v) => (showShortcuts = v)} />

    {#each sessions as session (session.id)}
      {@const tabs = getTabs(session.id)}
      {@const activeTab = getActiveTabIndex(session.id)}
      {@const hasDiff = diffTabOpen[session.id] ?? false}
      {@const isDiffActive = diffTabActive[session.id] ?? false}
      {@const project = projects.find((p) => p.id === session.project_id)}
      {#if session.id === activeSessionId && (tabs.length > 1 || hasDiff)}
        <div class="flex items-center h-8 bg-surface-100 dark:bg-surface-900 border-b border-surface-200 dark:border-surface-800 px-2 gap-0.5 shrink-0" role="tablist">
          <button
            role="tab"
            aria-selected={!isDiffActive}
            class="flex items-center gap-1 px-3 h-6 rounded text-xs select-none transition-colors
              {!isDiffActive ? 'bg-surface-200 dark:bg-surface-700 text-surface-900 dark:text-surface-50' : 'text-surface-600 dark:text-surface-400 hover:bg-surface-200/50 dark:hover:bg-surface-700/50'}"
            onclick={() => { diffTabActive = { ...diffTabActive, [session.id]: false }; }}
          >Terminal</button>
          {#if hasDiff}
            <button
              role="tab"
              aria-selected={isDiffActive}
              class="flex items-center gap-1 px-3 h-6 rounded text-xs select-none transition-colors
                {isDiffActive ? 'bg-surface-200 dark:bg-surface-700 text-surface-900 dark:text-surface-50' : 'text-surface-600 dark:text-surface-400 hover:bg-surface-200/50 dark:hover:bg-surface-700/50'}"
              onclick={() => { diffTabActive = { ...diffTabActive, [session.id]: true }; }}
            >
              <span>∆ Diff</span>
              <span
                class="ml-1 w-4 h-4 flex items-center justify-center rounded hover:bg-surface-300 dark:hover:bg-surface-600 text-[10px]"
                role="button"
                tabindex="-1"
                aria-label="Close diff"
                onclick={(e: MouseEvent) => { e.stopPropagation(); diffTabOpen = { ...diffTabOpen, [session.id]: false }; diffTabActive = { ...diffTabActive, [session.id]: false }; }}
              >×</span>
            </button>
          {/if}
        </div>
      {/if}
      {#each tabs as tab (tab.index)}
        {@const ptyKey = tab.index === 0 ? session.id : `${session.id}:${tab.index}`}
        <Terminal
          sessionId={ptyKey}
          visible={session.id === activeSessionId && tab.index === activeTab && !isDiffActive}
          focused={session.id === activeSessionId && tab.index === activeTab && !isDiffActive && zone === "terminal"}
          exited={tab.index === 0 && session.status === "exited"}
          skipAttach={tab.index !== 0}
          onAttached={() => {
            if (tab.index === 0 && session.status === "exited") {
              sessions = sessions.map((s) => s.id === session.id ? { ...s, status: "active" } : s);
              listenForExits();
            }
          }}
          onUserInput={() => {
            if (agentStates[session.id]) {
              const { [session.id]: _, ...rest } = agentStates;
              agentStates = rest;
            }
          }}
        />
      {/each}
      {#if hasDiff && project}
        {@const repoPath = session.worktree_path ?? project.path}
        {@const baseBranch = session.base_branch ?? "main"}
        <DiffTab
          {repoPath}
          {baseBranch}
          visible={session.id === activeSessionId && isDiffActive}
          theme={isDark() ? (getSettings().appearance.diff_theme_dark ?? "vs-dark") : (getSettings().appearance.diff_theme_light ?? "vs")}
        />
      {/if}
    {/each}

    {#if sessions.length === 0 && !showProjectForm && !showSessionForm}
      <div class="flex items-center justify-center h-full">
        <p class="text-surface-700 dark:text-surface-300">No active session. Press <kbd class="rounded border border-surface-300 dark:border-surface-600 px-1.5 py-0.5 text-xs">⌘N</kbd> to create one.</p>
      </div>
    {/if}

    {#if showHookPrompt}
      <div class="absolute top-2 left-4 right-4 z-20 flex items-center gap-3 rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-950 px-4 py-2.5 shadow-sm">
        <span class="text-sm text-amber-800 dark:text-amber-200">Install notification hook for instant agent-done alerts?</span>
        <button
          class="ml-auto rounded bg-amber-600 px-3 py-1 text-xs font-medium text-white hover:bg-amber-700"
          onclick={async () => { await invoke("install_notify_hook"); showHookPrompt = false; }}
        >Install</button>
        <button
          class="rounded px-2 py-1 text-xs text-surface-600 dark:text-surface-400 hover:text-surface-800 dark:hover:text-surface-200"
          onclick={() => (showHookPrompt = false)}
        >Dismiss</button>
      </div>
    {/if}

    {#if sessionToDelete}
      <Dialog.Root open={true} onOpenChange={(v) => { if (!v) sessionToDelete = null; }}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-50" />
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <Dialog.Content
            class="fixed left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none"
            onkeydown={(e) => { if (e.key === 'c' || e.key === 'n') sessionToDelete = null; if (e.key === 'd' || e.key === 'y') { const s = sessionToDelete; sessionToDelete = null; if (s) doDelete(s); } }}
          >
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
          <Dialog.Content
            class="fixed left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none"
            onkeydown={(e) => { if (e.key === 'c' || e.key === 'n') projectToDelete = null; if (e.key === 'd' || e.key === 'y') { deleteProject(ptd); } }}
          >
            <Dialog.Title class="text-sm">Delete project <strong>{ptd.name}</strong>?</Dialog.Title>
            <p class="text-xs text-surface-500 dark:text-surface-400">
              This will permanently remove {projSessions.length} session{projSessions.length !== 1 ? 's' : ''}{#if worktreeCount > 0} and clean up {worktreeCount} worktree{worktreeCount !== 1 ? 's' : ''}{/if}. This cannot be undone.
            </p>
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
          <Dialog.Content
            class="fixed left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none"
            onkeydown={(e) => { if (e.key === 'Escape' || e.key === 'n') showQuitConfirm = false; if (e.key === 'q' || e.key === 'y') { showQuitConfirm = false; getCurrentWindow().destroy(); } }}
          >
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
  </section>
  </div>
</main>

{#if getSnackbarMessage()}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed bottom-4 left-4 z-[100] max-w-lg cursor-pointer rounded-lg bg-red-600 px-4 py-3 shadow-lg"
    onclick={() => { navigator.clipboard.writeText(getSnackbarMessage()!); dismissSnackbar(); }}
    title="Click to copy and dismiss"
  >
    <p class="text-sm text-white font-mono break-all">{getSnackbarMessage()}</p>
    <p class="text-xs text-red-200 mt-1">Click to copy & dismiss</p>
  </div>
{/if}
