<script lang="ts">
  import type { FocusZone } from "../lib/focus.svelte";

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

  interface Props {
    projects: Project[];
    sessions: Session[];
    activeSessionId: string | null;
    zone: FocusZone;
    onAddProject: () => void;
    onSelectSession: (id: string) => void;
  }

  let { projects, sessions, activeSessionId, zone, onAddProject, onSelectSession }: Props = $props();

  const grouped = $derived(
    projects.map((p) => ({
      project: p,
      sessions: sessions.filter((s) => s.project_id === p.id),
    }))
  );
</script>

<aside
  class="w-56 border-r border-neutral-800 flex flex-col {zone === 'sidebar' ? 'bg-neutral-900' : 'bg-neutral-950'}"
>
  <div class="flex items-center justify-between p-3 pb-1">
    <h2 class="text-xs font-semibold text-neutral-500 uppercase tracking-wide">Sessions</h2>
    <button
      onclick={onAddProject}
      class="text-neutral-500 hover:text-neutral-300 text-lg leading-none"
      title="Add project"
    >+</button>
  </div>

  <div class="flex-1 overflow-y-auto p-3 pt-1">
    {#if projects.length === 0}
      <div class="mt-8 text-center">
        <p class="text-xs text-neutral-500 mb-2">No projects registered.</p>
        <button
          onclick={onAddProject}
          class="text-xs text-neutral-400 hover:text-neutral-200 underline"
        >
          Add a project
        </button>
      </div>
    {:else}
      {#each grouped as { project, sessions: projectSessions } (project.id)}
        <div class="mb-3">
          <p class="text-xs text-neutral-500 font-medium mb-1">{project.name}</p>
          {#each projectSessions as session (session.id)}
            <button
              class="w-full text-left px-2 py-1 rounded text-sm truncate {session.id === activeSessionId ? 'bg-neutral-700 text-neutral-100' : 'text-neutral-400 hover:bg-neutral-800'}"
              onclick={() => onSelectSession(session.id)}
            >
              {session.branch}
            </button>
          {/each}
          {#if projectSessions.length === 0}
            <p class="text-xs text-neutral-600 px-2">No sessions</p>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</aside>
