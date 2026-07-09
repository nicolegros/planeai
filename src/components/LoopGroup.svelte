<script lang="ts">
  import type { LoopRunSummary } from "../lib/types";
  import { ChevronDown, ChevronRight, Square, Play } from "@lucide/svelte";

  interface Props {
    loops: LoopRunSummary[];
    selectedLoopId: string | null;
    onSelectLoop: (id: string) => void;
    onTick: (id: string) => void;
    onStop: (id: string) => void;
  }

  let { loops, selectedLoopId, onSelectLoop, onTick, onStop }: Props = $props();

  let collapsed = $state(false);

  const statusColors: Record<string, string> = {
    draft: "bg-t3",
    running: "bg-status-running",
    observing: "bg-status-running",
    verifying: "bg-status-running",
    completed_unreviewed: "bg-status-review",
    blocked: "bg-status-exited",
    needs_human: "bg-status-review",
    stale: "bg-status-exited",
    failed: "bg-status-exited",
    cancelled: "bg-status-exited",
    approved: "bg-status-running",
    merged: "bg-status-idle",
    cleaned: "bg-status-idle",
  };

  function shortId(id: string): string {
    return id.slice(0, 8);
  }

  function statusColor(status: string): string {
    return statusColors[status] ?? "bg-t3";
  }

  function isActive(status: string): boolean {
    return ["running", "observing", "verifying"].includes(status);
  }
</script>

{#if loops.length > 0}
  <div class="space-y-0.5">
    <!-- Section header -->
    <button
      class="w-full px-2 text-[11px] font-semibold text-t2 uppercase tracking-[.05em] truncate flex items-center gap-1.5 rounded-lg py-1 hover:bg-panel-hi"
      onclick={() => (collapsed = !collapsed)}
    >
      Loops
      <span class="ml-auto font-normal text-t3">{loops.length}</span>
      {#if collapsed}<ChevronRight class="size-3 shrink-0 text-t3" />{:else}<ChevronDown class="size-3 shrink-0 text-t3" />{/if}
    </button>

    {#if !collapsed}
      <ul class="space-y-0.5" role="list" aria-label="Loop runs">
        {#each loops as loop (loop.id)}
          {@const selected = loop.id === selectedLoopId}
          <li>
            <button
              class="group w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-sm text-left transition-colors
                {selected ? 'bg-accent/10 text-accent' : 'text-t1 hover:bg-panel-hi'}"
              onclick={() => onSelectLoop(loop.id)}
              title={loop.goal}
            >
              <!-- Status dot -->
              <span class="size-2 rounded-full shrink-0 {statusColor(loop.status)}" aria-label="Status: {loop.status}"></span>

              <!-- Main content -->
              <span class="flex-1 min-w-0 truncate">
                {#if loop.task_key}
                  <span class="text-t2 font-medium">{loop.task_key}</span>
                {:else}
                  <span class="text-t3 font-mono text-xs">{shortId(loop.id)}</span>
                {/if}
                <span class="text-t3 text-xs ml-1">{loop.strategy}</span>
              </span>

              <!-- Round counter -->
              <span class="text-t3 text-xs shrink-0">{loop.current_round}/{loop.max_rounds}</span>

              <!-- Quick actions (show on hover) -->
              <span class="hidden group-hover:flex items-center gap-0.5">
                {#if isActive(loop.status)}
                  <button
                    class="p-0.5 rounded hover:bg-panel-hi text-t3 hover:text-t1"
                    onclick={(e) => { e.stopPropagation(); onTick(loop.id); }}
                    title="Tick"
                    aria-label="Tick loop"
                  >
                    <Play class="size-3" />
                  </button>
                  <button
                    class="p-0.5 rounded hover:bg-panel-hi text-t3 hover:text-status-exited"
                    onclick={(e) => { e.stopPropagation(); onStop(loop.id); }}
                    title="Stop"
                    aria-label="Stop loop"
                  >
                    <Square class="size-3" />
                  </button>
                {/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}
