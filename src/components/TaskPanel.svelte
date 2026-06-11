<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { focusTerminal } from "../lib/focus.svelte";
  import { Button, Input, Label, ContextMenu, Dialog, Select } from "./ui";
  import { Plus, RefreshCw, ChevronDown, ChevronRight, Lightbulb, LoaderCircle } from "@lucide/svelte";

  interface TaskItem {
    key: string;
    title: string;
    status: string;
    description: string;
    priority: number;
    blocked_by: string[];
    tags: string[];
    url: string | null;
  }

  interface Session {
    id: string;
    task_key: string | null;
  }

  interface Project {
    name: string;
    path: string;
  }

  interface Props {
    projects: Project[];
    sessions: Session[];
    agentStates: Record<string, string>;
    taskCreateRequested?: boolean;
    onPickTask: (task: TaskItem, repoPath: string) => void;
    onSelectSession: (id: string) => void;
    onArchiveSession?: (session: Session) => void | Promise<void>;
    onTaskCreateConsumed?: () => void;
  }

  let { projects, sessions, agentStates, taskCreateRequested = false, onPickTask, onSelectSession, onArchiveSession, onTaskCreateConsumed }: Props = $props();

  // React to external create request
  $effect(() => {
    if (taskCreateRequested) {
      openCreate();
      onTaskCreateConsumed?.();
    }
  });

  let tasksByProject = $state<Record<string, TaskItem[]>>({});
  let loading = $state(false);
  let collapsedSections = $state<Record<string, boolean>>({ done: true });

  // Modal state
  let modalMode = $state<"create" | "edit" | null>(null);
  let formTitle = $state("");
  let formDescription = $state("");
  let formPriority = $state(0);
  let formKey = $state("");
  let formProjectPath = $state("");

  // Context menu
  let contextMenu = $state<{ x: number; y: number; task: TaskItem } | null>(null);

  const statusOrder = ["in_progress", "in_review", "todo", "done"];
  const statusLabels: Record<string, string> = {
    in_progress: "In Progress",
    in_review: "In Review",
    todo: "Todo",
    done: "Done",
  };

  const statusColors: Record<string, string> = {
    todo: "text-blue-500 dark:text-blue-400",
    in_progress: "text-amber-500 dark:text-amber-400",
    in_review: "text-green-500 dark:text-green-400",
    done: "text-purple-500 dark:text-purple-400",
  };

  function groupByStatus(items: TaskItem[]): Record<string, TaskItem[]> {
    const groups: Record<string, TaskItem[]> = {};
    for (const s of statusOrder) groups[s] = [];
    for (const t of items) {
      (groups[t.status] ?? (groups["todo"] ??= [])).push(t);
    }
    for (const s of statusOrder) groups[s]?.sort((a, b) => b.priority - a.priority);
    return groups;
  }

  export async function refresh() {
    if (projects.length === 0) return;
    loading = true;
    try {
      const results: Record<string, TaskItem[]> = {};
      await Promise.all(
        projects.map(async (p) => {
          try {
            results[p.path] = await invoke<TaskItem[]>("list_all_task_items", { repoPath: p.path });
          } catch {
            results[p.path] = [];
          }
        })
      );
      tasksByProject = results;
    } finally {
      loading = false;
    }
  }

  $effect(() => { if (projects.length > 0) refresh(); });

  function toggleSection(key: string) {
    collapsedSections = { ...collapsedSections, [key]: !collapsedSections[key] };
  }

  function sessionForTask(key: string): Session | undefined {
    return sessions.find((s) => s.task_key === key);
  }

  function repoPathForTask(key: string): string | null {
    for (const [path, items] of Object.entries(tasksByProject)) {
      if (items.some((t) => t.key === key)) return path;
    }
    return projects[0]?.path ?? null;
  }

  function handleClick(task: TaskItem) {
    const linked = sessionForTask(task.key);
    if (linked) {
      onSelectSession(linked.id);
    } else {
      onPickTask(task, repoPathForTask(task.key) ?? "");
    }
  }

  function onContextMenuOpen(e: MouseEvent, task: TaskItem) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, task };
  }

  function openCreate() {
    formTitle = "";
    formDescription = "";
    formPriority = 0;
    formProjectPath = projects[0]?.path ?? "";
    modalMode = "create";
  }

  function openEdit(task: TaskItem) {
    formKey = task.key;
    formTitle = task.title;
    formDescription = task.description;
    formPriority = task.priority;
    modalMode = "edit";
  }

  async function moveTask(key: string, status: string) {
    const repoPath = repoPathForTask(key);
    if (!repoPath) return;
    try {
      await invoke("move_task_item", { key, status, repoPath });
      if (status === "done") {
        const linked = sessionForTask(key);
        if (linked) {
          console.log(`[task-panel] task ${key} → done, archiving session ${linked.id}`);
          try { await onArchiveSession?.(linked); } catch (e: any) { showSnackbar(`Archive failed: ${e}`); }
        } else {
          console.log(`[task-panel] task ${key} → done, no linked session`);
        }
      }
      await refresh();
    } catch (e: any) { showSnackbar(e.toString()); }
  }

  async function handleSubmit() {
    if (!formTitle.trim()) return;
    try {
      if (modalMode === "create") {
        const repoPath = formProjectPath || projects[0]?.path;
        if (!repoPath) return;
        await invoke("create_task_item", { repoPath, title: formTitle.trim(), description: formDescription, priority: formPriority, tags: [], blockedBy: [] });
      } else if (modalMode === "edit") {
        const repoPath = repoPathForTask(formKey);
        if (!repoPath) return;
        await invoke("edit_task_item", { repoPath, key: formKey, title: formTitle.trim(), description: formDescription, priority: formPriority, tags: null, blockedBy: null });
      }
      modalMode = null;
      await refresh();
    } catch (e: any) { showSnackbar(e.toString()); }
  }

  function autofocusForm(node: HTMLFormElement) {
    requestAnimationFrame(() => node.querySelector<HTMLInputElement>("input")?.focus());
  }

  function autoResize(node: HTMLTextAreaElement) {
    requestAnimationFrame(() => { node.style.height = 'auto'; node.style.height = node.scrollHeight + 'px'; });
  }

  function contextMenuItems(task: TaskItem) {
    const items: { label: string; onSelect: () => void; danger?: boolean }[] = [];
    const linked = sessionForTask(task.key);
    if (linked) {
      items.push({ label: "Go to session", onSelect: () => onSelectSession(linked.id) });
    } else {
      items.push({ label: "Start session", onSelect: () => onPickTask(task, repoPathForTask(task.key) ?? "") });
    }
    items.push({ label: "Edit", onSelect: () => openEdit(task) });
    if (task.status !== "in_progress") items.push({ label: "→ In Progress", onSelect: () => moveTask(task.key, "in_progress") });
    if (task.status !== "in_review") items.push({ label: "→ In Review", onSelect: () => moveTask(task.key, "in_review") });
    if (task.status !== "todo") items.push({ label: "→ Todo", onSelect: () => moveTask(task.key, "todo") });
    if (task.status !== "done") items.push({ label: "→ Done", onSelect: () => moveTask(task.key, "done") });
    return items;
  }
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="flex items-center justify-between px-4 py-3 border-b border-surface-200 dark:border-surface-800">
    <span class="text-xs font-semibold text-surface-700 dark:text-surface-300 uppercase tracking-wider">Tasks</span>
    <div class="flex items-center gap-1">
      <button onclick={() => refresh()} title="Refresh" class="size-6 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors">
        <RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
      </button>
      <button onclick={openCreate} title="Create task" class="size-6 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors">
        <Plus class="size-4" />
      </button>
    </div>
  </div>

  <!-- Task list: project > status -->
  <nav class="flex-1 overflow-y-auto px-2 py-2 space-y-3">
    {#if projects.length === 0}
      <p class="text-xs text-surface-600 dark:text-surface-400 text-center mt-8">No projects</p>
    {:else if Object.values(tasksByProject).every(t => t.length === 0) && !loading}
      <p class="text-xs text-surface-600 dark:text-surface-400 text-center mt-8">No tasks found</p>
    {:else}
      {#each projects as project (project.path)}
        {@const projectTasks = tasksByProject[project.path] ?? []}
        {#if projectTasks.length > 0}
          {@const statusGroups = groupByStatus(projectTasks)}
          <div>
            <h3 class="px-2 mb-1 text-[11px] font-semibold text-surface-600 dark:text-surface-400 uppercase tracking-wider truncate">{project.name}</h3>
            {#each statusOrder as status}
              {@const items = statusGroups[status] ?? []}
              {#if items.length > 0}
                {@const sectionKey = `${project.path}:${status}`}
                <div class="ml-1">
                  <button
                    class="w-full flex items-center gap-1.5 px-2 py-1 text-xs font-semibold {statusColors[status] ?? 'text-surface-500'} hover:opacity-80"
                    onclick={() => toggleSection(sectionKey)}
                  >
                    {#if collapsedSections[sectionKey]}
                      <ChevronRight class="size-3" />
                    {:else}
                      <ChevronDown class="size-3" />
                    {/if}
                    {statusLabels[status] ?? status}
                    <span class="font-normal ml-0.5">({items.length})</span>
                  </button>
                  {#if !collapsedSections[sectionKey]}
                    <ul class="space-y-0.5 ml-1">
                      {#each items as task (task.key)}
                        <li>
                          <button
                            class="w-full text-left px-2 py-1.5 rounded-md text-sm flex items-center gap-1.5 transition-colors hover:bg-surface-200 dark:hover:bg-surface-800 select-none"
                            onclick={() => handleClick(task)}
                            oncontextmenu={(e) => onContextMenuOpen(e, task)}
                          >
                            <span class="shrink-0 text-[10px] font-medium text-primary-600 dark:text-primary-400">{task.key}</span>
                            <span class="truncate text-xs text-surface-700 dark:text-surface-300">{task.title}</span>
                            {#if sessionForTask(task.key)}
                              {@const linked = sessionForTask(task.key)!}
                              {#if agentStates[linked.id] === 'Busy'}
                                <span class="ml-auto shrink-0 size-3.5 animate-spin text-surface-500" title="Agent working">
                                  <LoaderCircle class="size-3.5" />
                                </span>
                              {:else if agentStates[linked.id] === 'Idle'}
                                <span class="ml-auto shrink-0 size-3.5 animate-pulse text-amber-500" title="Needs attention">
                                  <Lightbulb class="size-3.5" />
                                </span>
                              {:else}
                                <span class="ml-auto shrink-0 size-1.5 rounded-full bg-primary-500"></span>
                              {/if}
                            {/if}
                          </button>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      {/each}
    {/if}
  </nav>
</div>

<!-- Context menu -->
{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={contextMenuItems(contextMenu.task)}
  />
{/if}

<!-- Modal for create/edit -->
<Dialog open={modalMode !== null} onOpenChange={(v) => { if (!v) { modalMode = null; focusTerminal(); } }} title={modalMode === "create" ? "Create Task" : "Edit Task"} class="w-[36rem] p-6">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    class="space-y-4"
    onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}
    onkeydown={(e) => { if (e.key === "Escape") { e.stopPropagation(); modalMode = null; focusTerminal(); } if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); handleSubmit(); } }}
    use:autofocusForm
  >
    <h2 class="text-lg font-semibold text-surface-900 dark:text-surface-50">{modalMode === "create" ? "Create Task" : "Edit Task"}</h2>

    {#if modalMode === "create" && projects.length > 1}
      <div class="space-y-1">
        <Label>Project</Label>
        <Select
          items={projects.map(p => ({ value: p.path, label: p.name }))}
          bind:value={formProjectPath}
          placeholder="Select project…"
        />
      </div>
    {/if}

    <div class="space-y-1">
      <Label>Title</Label>
      <Input bind:value={formTitle} placeholder="Task title" />
    </div>

    <div class="space-y-1">
      <Label>Description</Label>
      <textarea
        bind:value={formDescription}
        placeholder="Optional description"
        class="w-full rounded border border-surface-300 bg-surface-50 px-3 py-2 text-sm text-surface-900 placeholder:text-surface-400 dark:border-surface-600 dark:bg-surface-900 dark:text-surface-50 dark:placeholder:text-surface-500 resize-none min-h-[4rem] max-h-[50vh] overflow-y-auto"
        rows="3"
        oninput={(e) => { const el = e.currentTarget; el.style.height = 'auto'; el.style.height = el.scrollHeight + 'px'; }}
        use:autoResize
      ></textarea>
    </div>

    <div class="space-y-1">
      <Label>Priority</Label>
      <input type="number" bind:value={formPriority} class="w-20 rounded border border-surface-300 bg-surface-50 px-3 py-2 text-sm dark:border-surface-600 dark:bg-surface-900 dark:text-surface-50" />
    </div>

    <div class="flex justify-end gap-2">
      <Button type="button" onclick={() => { modalMode = null; focusTerminal(); }}>Cancel</Button>
      <Button type="submit" variant="primary" disabled={!formTitle.trim()}>{modalMode === "create" ? "Create" : "Save"} <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span></Button>
    </div>
  </form>
</Dialog>
