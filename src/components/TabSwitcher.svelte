<script lang="ts">
  import { Lightbulb } from "@lucide/svelte";
  import type { Session, Project } from "../lib/types";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";
  import { isTaskWorkspaceId, parseTaskWorkspaceId } from "../lib/sidebar-session-order";

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
    todo: "bg-t3",
    in_progress: "bg-accent",
    in_review: "bg-status-review",
    done: "bg-status-running",
  };

  function getSession(id: string) {
    return sessions.find((s) => s.id === id);
  }

  function getProjectName(projectId: string) {
    return projects.find((p) => p.id === projectId)?.name ?? "";
  }

  function getTaskWorkspace(id: string): { task: import("../lib/types").TaskItem; project: Project } | null {
    const parsed = parseTaskWorkspaceId(id);
    const project = parsed ? projects.find((candidate) => candidate.id === parsed.projectId) : undefined;
    const task = project && parsed ? taskStore.getTasksForProject(project.path).find((candidate) => candidate.key === parsed.taskKey) : undefined;
    return project && task ? { project, task } : null;
  }
</script>

<div class="absolute inset-0 flex items-center justify-center z-20">
  <div class="w-[512px] max-w-[84%] bg-panel border border-border-s rounded-xl shadow-[0_26px_70px_-14px_rgba(0,0,0,0.6)] p-2">
    {#each mruSessionIds as id, i (id)}
      {#if isTaskWorkspaceId(id)}
        {@const workspace = getTaskWorkspace(id)}
        {#if workspace}
          <div class="flex items-center gap-[10px] h-[38px] px-3 rounded-lg {i === selectedIndex ? 'bg-accent' : ''}">
            <span class="shrink-0 size-[6px] rounded-full {statusDotColors[workspace.task.status] ?? 'bg-t3'}"></span>
            <span class="shrink-0 font-mono text-[10px] font-medium {i === selectedIndex ? 'text-on-accent opacity-65' : 'text-t3'}">{workspace.task.key}</span>
            <span class="text-[13px] font-medium truncate {i === selectedIndex ? 'text-on-accent' : 'text-t1'}">{workspace.task.title}</span>
            <span class="ml-auto shrink-0 text-[11.5px] {i === selectedIndex ? 'text-on-accent opacity-65' : 'text-t3'}">{workspace.project.name}</span>
          </div>
        {/if}
      {:else}
        {@const session = getSession(id)}
        {#if session}
          <div class="flex items-center gap-[10px] h-[38px] px-3 rounded-lg {i === selectedIndex ? 'bg-accent' : ''}">
            {#if session.task_key && taskStatuses[session.task_key]}
              <span class="shrink-0 size-[6px] rounded-full {statusDotColors[taskStatuses[session.task_key]] ?? 'bg-t3'}"></span>
            {/if}
            {#if session.task_key}<span class="shrink-0 font-mono text-[10px] font-medium {i === selectedIndex ? 'text-on-accent opacity-65' : 'text-t3'}">{session.task_key}</span>{/if}
            <span class="text-[13px] font-medium truncate {i === selectedIndex ? 'text-on-accent' : 'text-t1'}">{session.name || session.branch}</span>
            <span class="ml-auto shrink-0 text-[11.5px] {i === selectedIndex ? 'text-on-accent opacity-65' : 'text-t3'}">{getProjectName(session.project_id)}</span>
            {#if agentStates[id] === 'Busy'}
              <span class="shrink-0 size-3 rounded-full border-2 border-border animate-spin {i === selectedIndex ? 'border-t-on-accent' : 'border-t-t2'}"></span>
            {:else if agentStates[id] === 'Idle'}
              <Lightbulb class="shrink-0 size-[14px] text-status-review animate-pulse" />
            {/if}
          </div>
        {/if}
      {/if}
    {/each}
  </div>
</div>
