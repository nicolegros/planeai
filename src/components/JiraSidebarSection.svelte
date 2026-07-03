<script lang="ts">
  import type { TaskItem } from "../lib/types";
  import { getSelectedIndex } from "../lib/sidebar-nav.svelte";
  import { ChevronDown, ChevronRight, CheckCircle2, Cable } from "@lucide/svelte";
  import { ContextMenu } from "./ui";

  interface Props {
    tasks: TaskItem[];
    childCounts: Record<string, number>;
    collapsed: boolean;
    zone: string;
    flatNavIndex: Map<string, number>;
    onToggleSection: () => void;
    onAssignJiraTask: (key: string) => void;
  }

  let { tasks, childCounts, collapsed, zone, flatNavIndex, onToggleSection, onAssignJiraTask }: Props = $props();

  let contextMenu = $state<{ x: number; y: number; task: TaskItem } | null>(null);

  const headerNavIdx = $derived(flatNavIndex.get("jira_header") ?? -1);
  const isHeaderSelected = $derived(zone === "sidebar" && headerNavIdx === getSelectedIndex());
</script>

{#if tasks.length > 0}
  <div>
    <button
      data-nav-index={headerNavIdx}
      class="w-full px-2 mb-1 text-[11px] font-semibold text-t2 uppercase tracking-[.05em] truncate flex items-center gap-1.5 rounded-lg py-1 hover:bg-panel-hi {isHeaderSelected ? 'ring-2 ring-accent' : ''}"
      onclick={onToggleSection}
    >
      {#if collapsed}<ChevronRight class="size-3 shrink-0 text-t3" />{:else}<ChevronDown class="size-3 shrink-0 text-t3" />{/if}
      <Cable class="size-3 shrink-0 text-t3" />
      Jira
      <span class="ml-auto font-normal text-t3">{tasks.length}</span>
    </button>

    {#if !collapsed}
      <ul class="space-y-0.5">
        {#each tasks as task (task.key)}
          {@const navIdx = flatNavIndex.get(`jira:${task.key}`) ?? -1}
          {@const isSelected = zone === "sidebar" && navIdx === getSelectedIndex()}
          {@const childCount = childCounts[task.key] ?? 0}
          <li>
            <div class="flex items-center gap-1.5">
              <button
                data-nav-index={navIdx}
                class="flex-1 min-w-0 text-left py-[6px] px-2 flex items-center gap-1.5 transition-colors rounded-lg hover:bg-panel-hi {isSelected ? 'ring-2 ring-accent' : ''}"
                onclick={() => onAssignJiraTask(task.key)}
                oncontextmenu={(e) => { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, task }; }}
              >
                {#if task.status === "done"}
                  <CheckCircle2 class="size-3 shrink-0 text-status-running" />
                {:else if task.status === "in_progress" || task.status === "in_review"}
                  <span class="size-2 shrink-0 rounded-full bg-status-running"></span>
                {:else}
                  <span class="size-2 shrink-0 rounded-full bg-t3"></span>
                {/if}
                <span class="shrink-0 font-mono text-[10px] text-t3">{task.key}</span>
                <span class="truncate text-[12.5px] {task.status === 'done' ? 'line-through text-t3' : 'text-t1'}">{task.title}</span>
                {#if childCount > 0}
                  <span class="ml-auto shrink-0 font-mono text-[9px] text-t3 bg-panel-hi rounded px-[4px] py-[1px]">{childCount}</span>
                {/if}
              </button>
            </div>
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
      { label: "Assign to project", onSelect: () => onAssignJiraTask(contextMenu!.task.key) },
    ]}
  />
{/if}
