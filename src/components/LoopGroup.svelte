<script lang="ts">
  import type { LoopRunSummary, LoopSessionItem, Session } from "../lib/types";
  import { ChevronDown, ChevronRight, Square, Play, LoaderCircle, Lightbulb } from "@lucide/svelte";
  import { ContextMenu } from "./ui";
  import * as orchestrator from "../lib/session-orchestrator.svelte";

  interface Props {
    loops: LoopRunSummary[];
    selectedLoopId: string | null;
    loopSessions: Record<string, LoopSessionItem[]>;
    sessions: Session[];
    activeSessionId: string | null;
    agentStates: Record<string, string>;
    onSelectLoop: (id: string) => void;
    onSelectSession: (id: string) => void;
    onStartLoop: (id: string) => void;
    onTick: (id: string) => void;
    onStop: (id: string) => void;
    onDelete: (id: string) => void;
  }

  let { loops, selectedLoopId, loopSessions, sessions, activeSessionId, agentStates, onSelectLoop, onSelectSession, onStartLoop, onTick, onStop, onDelete }: Props = $props();

  let collapsed = $state(false);
  /** Per-loop expansion state (default: expanded) */
  let expandedLoops = $state<Set<string>>(new Set(loops.map(l => l.id)));
  let contextMenu = $state<{ x: number; y: number; loop: LoopRunSummary } | null>(null);

  // Auto-expand new loops
  $effect(() => {
    const current = new Set(expandedLoops);
    let changed = false;
    for (const loop of loops) {
      if (!current.has(loop.id)) {
        current.add(loop.id);
        changed = true;
      }
    }
    if (changed) expandedLoops = current;
  });

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

  function toggleLoopExpanded(loopId: string) {
    const next = new Set(expandedLoops);
    if (next.has(loopId)) next.delete(loopId);
    else next.add(loopId);
    expandedLoops = next;
  }

  function getSessionsForLoop(loopId: string): { item: LoopSessionItem; session: Session }[] {
    const items = loopSessions[loopId] ?? [];
    return items
      .map((item) => {
        const session = sessions.find((s) => s.id === item.session_id);
        return session ? { item, session } : null;
      })
      .filter((x): x is { item: LoopSessionItem; session: Session } => x !== null);
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
          {@const isExpanded = expandedLoops.has(loop.id)}
          {@const childSessions = getSessionsForLoop(loop.id)}
          <li>
            <!-- Loop item -->
            <div class="flex items-center gap-0.5">
              <!-- Expand/collapse chevron -->
              {#if childSessions.length > 0}
                <button
                  class="p-0.5 rounded hover:bg-panel-hi text-t3"
                  onclick={() => toggleLoopExpanded(loop.id)}
                  aria-label={isExpanded ? "Collapse loop sessions" : "Expand loop sessions"}
                >
                  {#if isExpanded}<ChevronDown class="size-3" />{:else}<ChevronRight class="size-3" />{/if}
                </button>
              {:else}
                <span class="w-[20px]"></span>
              {/if}

              <button
                class="group flex-1 min-w-0 flex items-center gap-2 px-2 py-1.5 rounded-md text-sm text-left transition-colors
                  {selected ? 'bg-accent/10 text-accent' : 'text-t1 hover:bg-panel-hi'}"
                onclick={() => onSelectLoop(loop.id)}
                oncontextmenu={(e) => { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, loop }; }}
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
                  {#if loop.status === "draft"}
                    <button
                      class="p-0.5 rounded hover:bg-panel-hi text-t3 hover:text-t1"
                      onclick={(e) => { e.stopPropagation(); onStartLoop(loop.id); }}
                      title="Start"
                      aria-label="Start loop"
                    >
                      <Play class="size-3" />
                    </button>
                  {:else if isActive(loop.status)}
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
            </div>

            <!-- Indented child sessions -->
            {#if isExpanded && childSessions.length > 0}
              <ul class="space-y-0.5 mt-0.5">
                {#each childSessions as { item, session } (session.id)}
                  {@const isActiveSession = session.id === activeSessionId}
                  <li>
                    <div class="flex items-center gap-1.5">
                      <span class="w-[2px] self-stretch rounded-full transition-opacity {isActiveSession ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
                      <button
                        class="flex-1 min-w-0 text-left py-[6px] text-[13px] flex items-center gap-1.5 transition-colors rounded-lg pl-8 pr-2
                          {isActiveSession ? 'bg-accent-bg' : 'hover:bg-panel-hi'}"
                        onclick={() => onSelectSession(session.id)}
                      >
                        <span class="shrink-0 font-mono text-[10px] text-t3">{item.role}</span>
                        <span class="truncate font-medium text-t1">{session.name || session.branch}</span>
                        <span class="ml-auto shrink-0 flex items-center gap-1.5">
                          {#if orchestrator.getReviewReady()[session.id]}
                            <Lightbulb class="size-3.5 text-status-review animate-pulse" />
                          {:else if session.status === 'exited'}
                            <span class="font-mono text-[9px] text-t3 bg-panel-hi rounded px-[5px] py-[1px]">exited</span>
                          {:else if agentStates[session.id] === 'Busy'}
                            <LoaderCircle class="size-3 animate-spin text-t2" />
                          {/if}
                        </span>
                      </button>
                    </div>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={[
      ...(contextMenu.loop.status === "draft" ? [{ label: "Start loop", onSelect: () => onStartLoop(contextMenu!.loop.id) }] : []),
      ...(isActive(contextMenu.loop.status) ? [{ label: "Stop loop", onSelect: () => onStop(contextMenu!.loop.id) }] : []),
      { label: "Delete loop", danger: true, onSelect: () => onDelete(contextMenu!.loop.id) },
    ]}
  />
{/if}
