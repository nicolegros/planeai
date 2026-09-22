<script lang="ts">
  import type { Session, TaskItem } from "../lib/types";
  import { Button } from "./ui";

  interface Props {
    task: TaskItem;
    archivedSessions: Session[];
    active?: boolean;
    onNewSession: () => void;
    onRestore: (session: Session) => void;
    onEdit: () => void;
  }

  let { task, archivedSessions, active = true, onNewSession, onRestore, onEdit }: Props = $props();

  function onKeydown(event: KeyboardEvent): void {
    if (!active || event.metaKey || event.ctrlKey || event.altKey) return;
    const target = event.target;
    if (target instanceof Element && target.closest("input, textarea, [role='combobox'], [role='dialog']")) return;
    const key = event.key.toLowerCase();
    if (key === "n") { event.preventDefault(); onNewSession(); }
    else if (key === "r" && archivedSessions.length > 0) { event.preventDefault(); onRestore(archivedSessions[0]!); }
    else if (key === "e") { event.preventDefault(); onEdit(); }
  }

  function priorityLabel(priority: number | null | undefined): string {
    return priority === 1 ? "High" : priority === 2 ? "Medium" : priority === 3 ? "Low" : "None";
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="flex h-full min-h-0 flex-col bg-panel" data-empty-task-workspace>
  <header class="shrink-0 border-b border-border px-8 py-6">
    <p class="font-mono text-xs text-accent">{task.key}</p>
    <h2 class="mt-1 max-w-5xl text-xl font-semibold leading-snug text-t1">{task.title}</h2>
  </header>

  <div class="grid min-h-0 flex-1 grid-cols-1 gap-8 overflow-hidden px-8 py-7 lg:grid-cols-[minmax(0,1fr)_15rem]">
    <section class="min-h-0 overflow-y-auto pr-4">
      <h3 class="text-sm font-medium text-t1">Description</h3>
      {#if task.description}
        <p class="mt-3 whitespace-pre-wrap text-sm leading-6 text-t2">{task.description}</p>
      {:else}
        <p class="mt-3 text-sm text-t3">No description has been added to this task.</p>
      {/if}
    </section>

    <aside class="border-l border-border pl-6 text-sm">
      <dl class="space-y-4">
        <div><dt class="text-[10px] font-medium uppercase tracking-wider text-t3">Status</dt><dd class="mt-1 capitalize text-t1">{task.status.replaceAll("_", " ")}</dd></div>
        <div><dt class="text-[10px] font-medium uppercase tracking-wider text-t3">Priority</dt><dd class="mt-1 text-t1">{priorityLabel(task.priority)}</dd></div>
        <div><dt class="text-[10px] font-medium uppercase tracking-wider text-t3">Base branch</dt><dd class="mt-1 font-mono text-t1">{task.base_branch ?? "main"}</dd></div>
        <div><dt class="text-[10px] font-medium uppercase tracking-wider text-t3">Parent</dt><dd class="mt-1 text-t1">{task.parent_key ?? "None"}</dd></div>
        <div><dt class="text-[10px] font-medium uppercase tracking-wider text-t3">Blocked by</dt><dd class="mt-1 text-t1">{task.blocked_by.length ? task.blocked_by.join(", ") : "None"}</dd></div>
        <div><dt class="text-[10px] font-medium uppercase tracking-wider text-t3">Workspace</dt><dd class="mt-1 text-t1">0 active sessions<br />{archivedSessions.length} archived agent{archivedSessions.length === 1 ? "" : "s"}</dd></div>
      </dl>
    </aside>
  </div>

  <footer class="flex shrink-0 justify-end gap-2 border-t border-border bg-panel px-8 py-4">
      {#if archivedSessions.length > 0}
        <Button variant="primary" onclick={() => onRestore(archivedSessions[0]!)}>Restore {archivedSessions[0]!.name || archivedSessions[0]!.branch} <span class="ml-1 font-mono text-[10px] opacity-60">R</span></Button>
      {/if}
      <Button variant={archivedSessions.length > 0 ? "ghost" : "primary"} onclick={onNewSession}>Start agent session <span class="ml-1 font-mono text-[10px] opacity-60">N</span></Button>
      <Button onclick={onEdit}>Edit task <span class="ml-1 font-mono text-[10px] opacity-60">E</span></Button>
  </footer>
</div>
