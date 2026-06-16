<script lang="ts">
  import { LoaderCircle, Lightbulb } from "@lucide/svelte";
  import type { Session, Project } from "../lib/types";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    mruSessionIds: string[];
    selectedIndex: number;
  }

  let { mruSessionIds, selectedIndex }: Props = $props();

  const sessions = $derived(orchestrator.getSessions());
  const agentStates = $derived(orchestrator.getAgentStates());
  const projects = $derived(projectStore.getProjects());
  const taskStatuses = $derived(taskStore.getTaskStatuses());

  const statusDotColors: Record<string, string> = {
    todo: "bg-blue-500",
    in_progress: "bg-amber-500",
    in_review: "bg-green-500",
    done: "bg-purple-500",
  };

  function getSession(id: string) {
    return sessions.find((s) => s.id === id);
  }

  function getProjectName(projectId: string) {
    return projects.find((p) => p.id === projectId)?.name ?? "";
  }
</script>

<div class="absolute inset-0 flex items-center justify-center z-20">
  <div class="rounded-lg border border-surface-200 bg-surface-100 p-2 w-[32rem] max-h-screen overflow-y-auto shadow-lg dark:border-surface-800 dark:bg-surface-950">
    {#each mruSessionIds as id, i (id)}
      {@const session = getSession(id)}
      {#if session}
        <div class="px-3 py-2 rounded text-sm flex items-center gap-2 {i === selectedIndex ? 'bg-primary-500 text-surface-50' : 'text-surface-700 dark:text-surface-300'}">
          {#if session.task_key && taskStatuses[session.task_key]}
            <span class="shrink-0 size-2 rounded-full {statusDotColors[taskStatuses[session.task_key]] ?? 'bg-surface-400'}"></span>
          {/if}
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
