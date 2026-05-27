<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { focusTerminal, getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import { touchMru, removeMru } from "./lib/mru.svelte";
  import { getCycleState, startCycle, advance, commit, cancel } from "./lib/tab-switcher.svelte";
  import { loadSettings, getSettings, isDark } from "./lib/settings.svelte";
  import { getThemeById } from "./lib/terminal-themes";
  import { playTaskComplete } from "./lib/soundPlayer";
  import { Dialog } from "bits-ui";
  import Titlebar from "./components/Titlebar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import Terminal from "./components/Terminal.svelte";
  import TabSwitcher from "./components/TabSwitcher.svelte";
  import CommandMenu from "./components/CommandMenu.svelte";
  import PreferencesPage from "./components/PreferencesPage.svelte";

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
  }

  let projects = $state<Project[]>([]);
  let sessions = $state<Session[]>([]);
  let activeSessionId = $state<string | null>(null);
  let showProjectForm = $state(false);
  let sidebarVisible = $state(true);

  let showSessionForm = $state(false);

  // Command menu state
  let commandMenuOpen = $state(false);

  // Agent state tracking (Busy/Idle per session)
  let agentStates = $state<Record<string, string>>({});

  // Preferences state
  let showPreferences = $state(false);

  // Hook install prompt
  let showHookPrompt = $state(false);

  // Quit confirmation
  let showQuitConfirm = $state(false);
  let quitDirectCount = $state(0);

  const terminalBg = $derived.by(() => {
    const s = getSettings();
    const themeId = isDark() ? s.appearance.terminal_theme_dark : s.appearance.terminal_theme_light;
    return getThemeById(themeId).colors.background;
  });

  // Delete confirmation state
  let sessionToDelete = $state<Session | null>(null);

  // Rename state
  let renamingSessionId = $state<string | null>(null);

  async function doRename(id: string, name: string) {
    await invoke("rename_session", { id, name });
    sessions = sessions.map((s) => s.id === id ? { ...s, name } : s);
    renamingSessionId = null;
  }

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  async function loadSessions() {
    sessions = await invoke<Session[]>("list_sessions");
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

  onMount(() => {
    loadProjects();
    loadSessions();
    loadSettings();

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
        showPreferences = false;
        sessionToDelete = null;
        commandMenuOpen = false;
      } else if (action.type === "command_palette") {
        commandMenuOpen = !commandMenuOpen;
      } else if (action.type === "open_preferences") {
        showPreferences = !showPreferences;
      }
    },
    () => !showPreferences && !showSessionForm && !showProjectForm && !commandMenuOpen && !getCycleState().isCycling
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
      unlistenClose.then((fn) => fn());
      exitUnlisteners.forEach((fn) => fn());
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  });

  function onSessionCreated(session: Session) {
    showSessionForm = false;
    sessions = [...sessions, session];
    selectSession(session.id);
    focusTerminal();
    listenForExits();
  }

  async function doDelete(s: Session) {
    await invoke("destroy_session", { id: s.id });
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
      onOpenPreferences={() => (showPreferences = true)}
      onRenameSession={doRename}
      onStartRename={(id) => (renamingSessionId = id || null)}
    />
  {/if}

  <section class="flex-1 relative p-4 pr-0" style="background-color: {terminalBg}">
    {#if showPreferences}
      <PreferencesPage onBack={() => { showPreferences = false; focusTerminal(); }} />
    {:else}
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
        <Dialog.Overlay class="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-96 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-surface-200 bg-surface-50 p-6 shadow-lg dark:border-surface-700 dark:bg-surface-900">
          <Dialog.Title class="text-lg font-semibold mb-4">New Session</Dialog.Title>
          <SessionForm
            {projects}
            {sessions}
            onCreated={onSessionCreated}
            onCancel={() => (showSessionForm = false)}
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
    />

    {#each sessions as session (session.id)}
      <Terminal
        sessionId={session.id}
        visible={session.id === activeSessionId}
        focused={session.id === activeSessionId && zone === "terminal"}
        exited={session.status === "exited"}
        onUserInput={() => {
          if (agentStates[session.id]) {
            const { [session.id]: _, ...rest } = agentStates;
            agentStates = rest;
          }
        }}
      />
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
          <Dialog.Overlay class="fixed inset-0 z-50 bg-black/50" />
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
    {#if showQuitConfirm}
      <Dialog.Root open={true} onOpenChange={(v) => { if (!v) showQuitConfirm = false; }}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-50 bg-black/50" />
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
    {/if}
  </section>
  </div>
</main>
