<script lang="ts">
  import { LoaderCircle, Lightbulb } from "@lucide/svelte";
  import type { Session, Project } from "../lib/types";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";
  import * as loopStore from "../lib/loop-store.svelte";
  import { isLoopId, parseLoopId } from "../lib/sidebar-session-order";

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

  const loopStatusColors: Record<string, string> = {
    draft: "bg-t3", running: "bg-status-running", observing: "bg-status-running",
    verifying: "bg-status-running", completed_unreviewed: "bg-status-review",
    blocked: "bg-status-exited", needs_human: "bg-status-review",
    failed: "bg-status-exited", cancelled: "bg-status-exited",
    approved: "bg-status-running", merged: "bg-status-idle", cleaned: "bg-status-idle",
    stale: "bg-status-exited",
  };

  function getSession(id: string) {
    return sessions.find((s) => s.id === id);
  }

  function getProjectName(projectId: string) {
    return projects.find((p) => p.id === projectId)?.name ?? "";
  }

  function getLoop(id: string) {
    const loopId = parseLoopId(id);
    for (const p of projects) {
      const loop = loopStore.getLoopsForProject(p.id).find((l) => l.id === loopId);
      if (loop) return loop;
    }
    return null;
  }

  function shortId(id: string): string {
    return id.slice(0, 8);
  }
</script>

<div class="absolute inset-0 flex items-center justify-center z-20">
  <div class="w-[512px] max-w-[84%] bg-panel border border-border-s rounded-xl shadow-[0_26px_70px_-14px_rgba(0,0,0,0.6)] p-2">
    {#each mruSessionIds as id, i (id)}
      {#if isLoopId(id)}
        {@const loop = getLoop(id)}
        {#if loop}
          <div class="flex items-center gap-[10px] h-[38px] px-3 rounded-lg {i === selectedIndex ? 'bg-accent' : ''}">
            <span class="shrink-0 size-[6px] rounded-full {loopStatusColors[loop.status] ?? 'bg-t3'}"></span>
            <span class="text-[13px] font-medium truncate {i === selectedIndex ? 'text-on-accent' : 'text-t1'}">
              {loop.task_key ?? shortId(loop.id)}
            </span>
            <span class="text-[11px] {i === selectedIndex ? 'text-on-accent opacity-65' : 'text-t3'}">
              {loop.strategy} · max {loop.max_rounds} rounds
            </span>
            <span class="ml-auto shrink-0 text-[11.5px] {i === selectedIndex ? 'text-on-accent opacity-65' : 'text-t3'}">{getProjectName(loop.project_id)}</span>
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
