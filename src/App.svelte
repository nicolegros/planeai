<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { focusTerminal, getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import { getMruList, touchMru, removeMru } from "./lib/mru.svelte";
  import { Dialog } from "bits-ui";
  import Sidebar from "./components/Sidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import Terminal from "./components/Terminal.svelte";
  import TabSwitcher from "./components/TabSwitcher.svelte";
  import CommandMenu from "./components/CommandMenu.svelte";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Session {
    id: string;
    project_id: string;
    name: string;
    tmux_name: string;
    branch: string;
    status: string;
    created_at: string;
    worktree_path: string | null;
  }

  let projects = $state<Project[]>([]);
  let sessions = $state<Session[]>([]);
  let activeSessionId = $state<string | null>(null);
  let showProjectForm = $state(false);
  let sidebarVisible = $state(true);

  let showSessionForm = $state(false);

  // Tab switcher state
  let tabSwitcherOpen = $state(false);
  let tabSwitcherIndex = $state(0);

  // Command menu state
  let commandMenuOpen = $state(false);

  // Delete confirmation state
  let sessionToDelete = $state<Session | null>(null);

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  async function loadSessions() {
    sessions = await invoke<Session[]>("list_sessions");
    // On initial load, activate the first session (reconnection)
    if (sessions.length > 0 && !activeSessionId) {
      // Seed MRU with all sessions (active one first)
      for (let i = sessions.length - 1; i >= 0; i--) {
        touchMru(sessions[i].id);
      }
      selectSession(sessions[0].id);
    }
  }

  function selectSession(id: string) {
    activeSessionId = id;
    touchMru(id);
  }

  function jumpToSession(index: number) {
    if (index < sessions.length) {
      selectSession(sessions[index].id);
    }
  }

  onMount(() => {
    loadProjects();
    loadSessions();

    const cleanup = installKeyboardRouter((action) => {
      if (action.type === "new_session") {
        if (projects.length === 0) {
          showProjectForm = true;
        } else {
          showSessionForm = true;
        }
      } else if (action.type === "toggle_sidebar") {
        sidebarVisible = !sidebarVisible;
      } else if (action.type === "jump_to_session") {
        jumpToSession(action.index);
      } else if (action.type === "tab_switch") {
        if (!tabSwitcherOpen) {
          tabSwitcherOpen = true;
          tabSwitcherIndex = 1;
        } else {
          tabSwitcherIndex = (tabSwitcherIndex + 1) % mruList.length;
        }
      } else if (action.type === "tab_switch_reverse") {
        if (!tabSwitcherOpen) {
          tabSwitcherOpen = true;
          tabSwitcherIndex = mruList.length - 1;
        } else {
          tabSwitcherIndex = (tabSwitcherIndex - 1 + mruList.length) % mruList.length;
        }
      } else if (action.type === "focus_terminal") {
        if (tabSwitcherOpen) {
          tabSwitcherOpen = false;
        }
        showSessionForm = false;
        showProjectForm = false;
        sessionToDelete = null;
        commandMenuOpen = false;
      } else if (action.type === "command_palette") {
        commandMenuOpen = !commandMenuOpen;
      }
    });

    // Listen for Ctrl release to confirm tab switch
    function onKeyUp(e: KeyboardEvent) {
      if (tabSwitcherOpen && e.key === "Control") {
        const mru = getMruList();
        if (mru[tabSwitcherIndex]) {
          selectSession(mru[tabSwitcherIndex]);
        }
        tabSwitcherOpen = false;
        focusTerminal();
      }
    }
    window.addEventListener("keyup", onKeyUp);

    return () => {
      cleanup();
      window.removeEventListener("keyup", onKeyUp);
    };
  });

  function onSessionCreated(session: Session) {
    showSessionForm = false;
    sessions = [...sessions, session];
    selectSession(session.id);
    focusTerminal();
  }

  async function confirmDelete() {
    if (!sessionToDelete) return;
    const s = sessionToDelete;
    await invoke("destroy_session", { id: s.id, tmuxName: s.tmux_name });
    sessions = sessions.filter((x) => x.id !== s.id);
    removeMru(s.id);
    if (activeSessionId === s.id) {
      activeSessionId = sessions[0]?.id ?? null;
      if (activeSessionId) touchMru(activeSessionId);
    }
    sessionToDelete = null;
  }

  async function archiveCurrentSession() {
    if (!activeSessionId) return;
    const s = sessions.find((x) => x.id === activeSessionId);
    if (!s) return;
    await archiveSession(s);
  }

  async function archiveSession(s: Session) {
    await invoke("archive_session", { id: s.id, tmuxName: s.tmux_name });
    sessions = sessions.filter((x) => x.id !== s.id);
    removeMru(s.id);
    if (activeSessionId === s.id) {
      activeSessionId = sessions[0]?.id ?? null;
      if (activeSessionId) touchMru(activeSessionId);
    }
  }

  function deleteCurrentSession() {
    if (!activeSessionId) return;
    const s = sessions.find((x) => x.id === activeSessionId);
    if (s) sessionToDelete = s;
  }

  const zone = $derived(getActiveZone());
  const mruList = $derived(getMruList());
</script>

<main class="flex h-screen">
  {#if sidebarVisible}
    <Sidebar
      {projects}
      {sessions}
      {activeSessionId}
      {zone}
      onAddProject={() => (showProjectForm = true)}
      onSelectSession={selectSession}
      onArchiveSession={(s) => archiveSession(s)}
      onDeleteSession={(s) => (sessionToDelete = s)}
    />
  {/if}

  <section class="flex-1 relative">
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
        <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-96 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-gray-200 bg-white p-6 shadow-lg">
          <Dialog.Title class="text-lg font-semibold mb-4">New Session</Dialog.Title>
          <SessionForm
            {projects}
            onCreated={onSessionCreated}
            onCancel={() => (showSessionForm = false)}
          />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>

    {#if tabSwitcherOpen}
      <TabSwitcher
        mruSessionIds={mruList}
        {sessions}
        {projects}
        selectedIndex={tabSwitcherIndex}
      />
    {/if}

    <CommandMenu
      open={commandMenuOpen}
      {sessions}
      {activeSessionId}
      onOpenChange={(v) => (commandMenuOpen = v)}
      onSelectSession={(id) => { selectSession(id); focusTerminal(); }}
      onArchiveSession={archiveCurrentSession}
      onDeleteSession={deleteCurrentSession}
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
        tmuxName={session.tmux_name}
        visible={session.id === activeSessionId}
        focused={session.id === activeSessionId && zone === "terminal"}
      />
    {/each}

    {#if sessions.length === 0 && !showProjectForm && !showSessionForm}
      <div class="flex items-center justify-center h-full">
        <p class="text-gray-500">No active session. Press <kbd class="rounded border border-gray-300 px-1.5 py-0.5 text-xs">⌘N</kbd> to create one.</p>
      </div>
    {/if}

    {#if sessionToDelete}
      <div class="absolute inset-0 flex items-center justify-center bg-black/50 z-30">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          class="rounded-lg border border-gray-200 bg-white p-6 w-80 space-y-4 shadow-lg"
          onkeydown={(e) => { if (e.key === 'Escape') sessionToDelete = null; }}
        >
          <p class="text-sm">Delete session <strong>{sessionToDelete.name || sessionToDelete.branch}</strong>? This will kill the agent.</p>
          <div class="flex justify-end gap-2">
            <button class="rounded border border-gray-300 px-3 py-1.5 text-sm" onclick={() => (sessionToDelete = null)}>Cancel</button>
            <button class="rounded bg-red-600 px-3 py-1.5 text-sm text-white" onclick={confirmDelete} autofocus>Delete</button>
          </div>
        </div>
      </div>
    {/if}
  </section>
</main>
