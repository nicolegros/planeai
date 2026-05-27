<script lang="ts">
  interface Session {
    id: string;
    project_id: string;
    name: string;
    tmux_name: string | null;
    branch: string;
    status: string;
    created_at: string;
    backend: string;
  }

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Props {
    mruSessionIds: string[];
    sessions: Session[];
    projects: Project[];
    selectedIndex: number;
    agentStates: Record<string, string>;
  }

  let { mruSessionIds, sessions, projects, selectedIndex, agentStates }: Props = $props();

  function getSession(id: string) {
    return sessions.find((s) => s.id === id);
  }

  function getProjectName(projectId: string) {
    return projects.find((p) => p.id === projectId)?.name ?? "";
  }
</script>

<div class="absolute inset-0 flex items-center justify-center z-20">
  <div class="rounded-lg border border-surface-200 bg-surface-100 p-2 w-72 max-h-80 overflow-y-auto shadow-lg dark:border-surface-800 dark:bg-surface-950">
    {#each mruSessionIds as id, i (id)}
      {@const session = getSession(id)}
      {#if session}
        <div class="px-3 py-2 rounded text-sm flex items-center gap-2 {i === selectedIndex ? 'bg-primary-500 text-surface-50' : 'text-surface-700 dark:text-surface-300'}">
          <span class="font-medium truncate">{session.name || session.branch}</span>
          <span class="text-xs {i === selectedIndex ? 'text-surface-200' : 'text-surface-600 dark:text-surface-400'}">{getProjectName(session.project_id)}</span>
          {#if agentStates[id] === 'Busy'}
            <span class="ml-auto shrink-0 size-3.5 animate-spin {i === selectedIndex ? 'text-surface-200' : 'text-surface-500'}">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48-8.48l2.83-2.83M2 12h4m12 0h4m-3.93 7.07l-2.83-2.83M7.76 7.76L4.93 4.93"/></svg>
            </span>
          {:else if agentStates[id] === 'Idle'}
            <span class="ml-auto shrink-0 size-3.5 animate-pulse text-amber-500">
              <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a7 7 0 0 0-7 7c0 3.53 2.13 6.5 4 8.5V19a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1v-1.5c1.87-2 4-4.97 4-8.5a7 7 0 0 0-7-7zm-1 19h2a1 1 0 0 1 0 2h-2a1 1 0 0 1 0-2z"/></svg>
            </span>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
</div>
