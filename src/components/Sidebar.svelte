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
    onDeleteSession: (session: Session) => void;
  }

  let { projects, sessions, activeSessionId, zone, onAddProject, onSelectSession, onDeleteSession }: Props = $props();

  let selectedIndex = $state(0);

  const grouped = $derived(
    projects.map((p) => ({
      project: p,
      sessions: sessions.filter((s) => s.project_id === p.id),
    }))
  );

  const flatSessionIds = $derived(sessions.map((s) => s.id));

  $effect(() => {
    if (selectedIndex >= flatSessionIds.length) {
      selectedIndex = Math.max(0, flatSessionIds.length - 1);
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (zone !== "sidebar" || flatSessionIds.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, flatSessionIds.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      onSelectSession(flatSessionIds[selectedIndex]);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="w-56 border-r border-surface-800 flex flex-col bg-surface-950 {zone === 'sidebar' ? 'bg-surface-900' : ''}">
  <div class="flex items-center justify-between p-3 pb-1">
    <h2 class="text-xs font-semibold text-surface-500 uppercase tracking-wide">Sessions</h2>
    <button onclick={onAddProject} class="btn-icon btn-icon-sm preset-tonal-surface" title="Add project">+</button>
  </div>

  <div class="flex-1 overflow-y-auto p-3 pt-1">
    {#if projects.length === 0}
      <div class="mt-8 text-center space-y-2">
        <p class="text-xs text-surface-500">No projects registered.</p>
        <button onclick={onAddProject} class="btn btn-sm preset-tonal-primary">Add a project</button>
      </div>
    {:else}
      {#each grouped as { project, sessions: projectSessions } (project.id)}
        <div class="mb-3">
          <p class="text-xs text-surface-500 font-medium mb-1">{project.name}</p>
          {#each projectSessions as session (session.id)}
            {@const globalIndex = flatSessionIds.indexOf(session.id)}
            <div class="flex items-center group">
              <button
                class="flex-1 text-left px-2 py-1 rounded text-sm truncate
                  {session.id === activeSessionId ? 'preset-filled-surface-500' : 'text-surface-400 hover:bg-surface-800'}
                  {zone === 'sidebar' && globalIndex === selectedIndex ? 'ring-1 ring-primary-500' : ''}"
                onclick={() => onSelectSession(session.id)}
              >
                {session.branch}
              </button>
              <button
                class="px-1 text-surface-600 hover:text-error-400 opacity-0 group-hover:opacity-100 text-xs"
                onclick={(e) => { e.stopPropagation(); onDeleteSession(session); }}
                title="Delete session"
              >✕</button>
            </div>
          {/each}
          {#if projectSessions.length === 0}
            <p class="text-xs text-surface-600 px-2">No sessions</p>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</aside>
