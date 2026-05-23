<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";
  import Sidebar from "./components/Sidebar.svelte";
  import ProjectForm from "./components/ProjectForm.svelte";
  import SessionForm from "./components/SessionForm.svelte";
  import Terminal from "./components/Terminal.svelte";

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

  async function loadProjects() {
    projects = await invoke<Project[]>("list_projects");
  }

  async function loadSessions() {
    sessions = await invoke<Session[]>("list_sessions");
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
      }
    });
    return cleanup;
  });

  function onSessionCreated(session: Session) {
    showSessionForm = false;
    sessions = [...sessions, session];
    activeSessionId = session.id;
  }

  const zone = $derived(getActiveZone());
</script>

<main class="flex h-screen">
  <Sidebar
    {projects}
    {sessions}
    {activeSessionId}
    {zone}
    onAddProject={() => (showProjectForm = true)}
    onSelectSession={(id) => (activeSessionId = id)}
  />

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
