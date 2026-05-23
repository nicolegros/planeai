<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import { getMruList, touchMru } from "./lib/mru.svelte";
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

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  async function loadSessions() {
    sessions = await invoke<Session[]>("list_sessions");
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
  </section>
</main>
