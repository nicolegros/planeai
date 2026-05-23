<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import { getMruList, touchMru, removeMru } from "./lib/mru.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import Terminal from "./components/Terminal.svelte";
  import TabSwitcher from "./components/TabSwitcher.svelte";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Session {
    id: string;
    project_id: string;
    tmux_name: string;
    branch: string;
    status: string;
    created_at: string;
  }

  let projects = $state<Project[]>([]);
  let sessions = $state<Session[]>([]);
  let activeSessionId = $state<string | null>(null);
  let showProjectForm = $state(false);
  let showSessionForm = $state(false);
  let sidebarVisible = $state(true);

  // Tab switcher state
  let tabSwitcherOpen = $state(false);
  let tabSwitcherIndex = $state(0);

  // Delete confirmation state
  let sessionToDelete = $state<Session | null>(null);

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  async function loadSessions() {
    sessions = await invoke<Session[]>("list_sessions");
    // On initial load, activate the first session (reconnection)
    if (sessions.length > 0 && !activeSessionId) {
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
          tabSwitcherIndex = 1; // Start at second item (previous session)
        } else {
          tabSwitcherIndex = Math.min(tabSwitcherIndex + 1, mruList.length - 1);
        }
      } else if (action.type === "tab_switch_reverse") {
        if (tabSwitcherOpen) {
          tabSwitcherIndex = Math.max(tabSwitcherIndex - 1, 0);
        }
      } else if (action.type === "focus_terminal") {
        if (tabSwitcherOpen) {
          tabSwitcherOpen = false;
        }
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
    {:else if showSessionForm}
      <div class="absolute inset-0 flex items-center justify-center bg-black/50 z-10">
        <SessionForm
          {projects}
          onCreated={onSessionCreated}
          onCancel={() => (showSessionForm = false)}
        />
      </div>
    {/if}

    {#if tabSwitcherOpen}
      <TabSwitcher
        mruSessionIds={mruList}
        {sessions}
        {projects}
        selectedIndex={tabSwitcherIndex}
      />
    {/if}

    {#each sessions as session (session.id)}
      <Terminal
        sessionId={session.id}
        tmuxName={session.tmux_name}
        visible={session.id === activeSessionId}
      />
    {/each}

    {#if sessions.length === 0 && !showProjectForm && !showSessionForm}
      <div class="flex items-center justify-center h-full">
        <p class="text-neutral-500">No active session. Press <kbd class="px-1 bg-neutral-800 rounded">⌘N</kbd> to create one.</p>
      </div>
    {/if}

    {#if sessionToDelete}
      <div class="absolute inset-0 flex items-center justify-center bg-black/50 z-30">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          class="bg-neutral-900 border border-neutral-700 rounded-lg p-4 w-80"
          onkeydown={(e) => { if (e.key === 'Escape') sessionToDelete = null; }}
        >
          <p class="text-sm text-neutral-200 mb-3">Delete session <strong>{sessionToDelete.branch}</strong>? This will kill the agent.</p>
          <div class="flex justify-end gap-2">
            <button
              class="px-3 py-1 text-xs text-neutral-400 hover:text-neutral-200"
              onclick={() => (sessionToDelete = null)}
            >Cancel</button>
            <button
              class="px-3 py-1 text-xs bg-red-900 text-red-200 rounded hover:bg-red-800"
              onclick={confirmDelete}
            >Delete</button>
          </div>
        </div>
      </div>
    {/if}
  </section>
</main>
