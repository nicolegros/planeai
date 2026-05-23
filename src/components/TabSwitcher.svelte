<script lang="ts">
  interface Session {
    id: string;
    project_id: string;
    tmux_name: string;
    branch: string;
    status: string;
    created_at: string;
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
  }

  let { mruSessionIds, sessions, projects, selectedIndex }: Props = $props();

  function getSession(id: string) {
    return sessions.find((s) => s.id === id);
  }

  function getProjectName(projectId: string) {
    return projects.find((p) => p.id === projectId)?.name ?? "";
  }
</script>

<div class="absolute inset-0 flex items-center justify-center bg-black/40 z-20">
  <div class="bg-neutral-900 border border-neutral-700 rounded-lg p-2 w-72 max-h-80 overflow-y-auto">
    {#each mruSessionIds as id, i (id)}
      {@const session = getSession(id)}
      {#if session}
        <div
          class="px-3 py-2 rounded text-sm {i === selectedIndex ? 'bg-neutral-700 text-neutral-100' : 'text-neutral-400'}"
        >
          <span class="font-medium">{session.branch}</span>
          <span class="text-xs text-neutral-500 ml-2">{getProjectName(session.project_id)}</span>
        </div>
      {/if}
    {/each}
  </div>
</div>
