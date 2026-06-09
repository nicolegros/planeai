<script lang="ts">
  import { LoaderCircle, Lightbulb } from "@lucide/svelte";

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
  <div class="rounded-lg border border-surface-200 bg-surface-100 p-2 w-96 max-h-screen overflow-y-auto shadow-lg dark:border-surface-800 dark:bg-surface-950">
    {#each mruSessionIds as id, i (id)}
      {@const session = getSession(id)}
      {#if session}
        <div class="px-3 py-2 rounded text-sm flex items-center gap-2 {i === selectedIndex ? 'bg-primary-500 text-surface-50' : 'text-surface-700 dark:text-surface-300'}">
          {#if session.task_key}<span class="shrink-0 text-[10px] font-medium {i === selectedIndex ? 'text-surface-200' : 'text-primary-600 dark:text-primary-400'}">{session.task_key}</span>{/if}
          <span class="font-medium truncate">{session.name || session.branch}</span>
          <span class="ml-auto shrink-0 text-xs {i === selectedIndex ? 'text-surface-200' : 'text-surface-600 dark:text-surface-400'}">{getProjectName(session.project_id)}</span>
          {#if agentStates[id] === 'Busy'}
            <span class="shrink-0 size-3.5 animate-spin {i === selectedIndex ? 'text-surface-200' : 'text-surface-500'}">
              <LoaderCircle class="size-3.5" />
            </span>
          {:else if agentStates[id] === 'Idle'}
            <span class="shrink-0 size-3.5 animate-pulse text-amber-500">
              <Lightbulb class="size-3.5" />
            </span>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
</div>
