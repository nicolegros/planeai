<script lang="ts">
  import type { TaskItem, Session, Project } from "../lib/types";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { focusTerminal, getActiveZone } from "../lib/focus.svelte";
  import { getSelectedIndex, setSelectedIndex, clampIndex, handleSidebarKey } from "../lib/sidebar-nav.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { Button, Input, Label, ContextMenu, Select } from "./ui";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { ChevronDown, ChevronRight, Lightbulb, LoaderCircle } from "@lucide/svelte";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    onPickTask: (task: TaskItem, repoPath: string) => void;
    onSelectSession: (id: string) => void;
    onArchiveSession?: (session: Pick<Session, "id" | "task_key" | "pr_url">) => void | Promise<void>;
    onSessionsChanged?: () => void;
    disableKeyboard?: boolean;
  }

  let { disableKeyboard = false, onPickTask, onSelectSession, onArchiveSession, onSessionsChanged }: Props = $props();

  // ─── Derived from stores ────────────────────────────────────────────────────
  const projects = $derived(projectStore.getProjects().map(p => ({ name: p.name, path: p.path })));
  const sessions = $derived(orchestrator.getSessions());
  const activeSessionId = $derived(orchestrator.getActiveSessionId());
  const agentStates = $derived(orchestrator.getAgentStates());
  const tasksByProject = $derived(taskStore.getTasksByProject());
  const loading = $derived(taskStore.isLoading());
  let collapsedSections = $state<Record<string, boolean>>({ done: true });

  const activeTaskKey = $derived(
    activeSessionId ? sessions.find(s => s.id === activeSessionId)?.task_key ?? null : null
  );

  // Modal state
  let modalMode = $state<"create" | "edit" | null>(null);
  let formTitle = $state("");
  let formDescription = $state("");
  let formPriority = $state(0);
  let formKey = $state("");
  let formProjectPath = $state("");
  let formBaseBranch = $state("main");
  let taskFormWrapper = $state<HTMLDivElement | null>(null);

  const taskFk = createFormKeyboardController(
    () => [
      { key: "t", ref: () => taskFormWrapper?.querySelector<HTMLElement>("[data-field='title'] input") ?? null },
      { key: "d", ref: () => taskFormWrapper?.querySelector<HTMLElement>("[data-field='desc'] textarea") ?? null },
      { key: "p", ref: () => taskFormWrapper?.querySelector<HTMLElement>("[data-field='priority'] input") ?? null },
      { key: "b", ref: () => taskFormWrapper?.querySelector<HTMLElement>("[data-field='base'] input") ?? null },
    ],
    { wrapper: () => taskFormWrapper, onDismiss: () => { modalMode = null; focusTerminal(); } },
  );

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

  function isParentTask(task: TaskItem, allTasks: TaskItem[]): boolean {
    return allTasks.some(t => t.parent_key === task.key);
  }

  function groupByStatus(items: TaskItem[]): Record<string, TaskItem[]> {
    const groups: Record<string, TaskItem[]> = {};
    for (const s of statusOrder) groups[s] = [];
    for (const t of items) {
      (groups[t.status] ?? (groups["todo"] ??= [])).push(t);
    }
    for (const s of statusOrder) groups[s]?.sort((a, b) => b.priority - a.priority);
    return groups;
  }

  function toggleSection(key: string) {
    collapsedSections = { ...collapsedSections, [key]: !collapsedSections[key] };
  }

  function sessionForTask(key: string): Pick<Session, "id" | "task_key" | "pr_url"> | undefined {
    return sessions.find((s) => s.task_key === key);
  }

  function repoPathForTask(key: string): string | null {
    for (const [path, items] of Object.entries(tasksByProject)) {
      if (items.some((t) => t.key === key)) return path;
    }
    return projects[0]?.path ?? null;
  }

  function handleClick(task: TaskItem, projectPath: string) {
    // Parents are not pickable
    if (isParentTask(task, tasksByProject[projectPath] ?? [])) return;
    const linked = sessionForTask(task.key);
    if (linked) {
      onSelectSession(linked.id);
      focusTerminal();
    } else {
      onPickTask(task, repoPathForTask(task.key) ?? "");
    }
  }

  function onContextMenuOpen(e: MouseEvent, task: TaskItem) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, task };
  }

  export function openCreate() {
    formTitle = "";
    formDescription = "";
    formPriority = 0;
    formBaseBranch = "main";
    formProjectPath = projects[0]?.path ?? "";
    modalMode = "create";
  }

  export function openEdit(task: TaskItem) {
    formKey = task.key;
    formTitle = task.title;
    formDescription = task.description;
    formPriority = task.priority;
    formBaseBranch = task.base_branch;
    modalMode = "edit";
  }

  async function moveTask(key: string, status: string) {
    const repoPath = repoPathForTask(key);
    if (!repoPath) return;
    try {
      await taskStore.moveTask(key, status, repoPath);
      onSessionsChanged?.();
    } catch (e: any) { showSnackbar(e.toString()); }
  }

  async function handleSubmit() {
    if (!formTitle.trim()) return;
    try {
      if (modalMode === "create") {
        const repoPath = formProjectPath || projects[0]?.path;
        if (!repoPath) return;
        await taskStore.createTask({ repoPath, title: formTitle.trim(), description: formDescription, priority: formPriority, tags: [], blockedBy: [], baseBranch: formBaseBranch });
      } else if (modalMode === "edit") {
        const repoPath = repoPathForTask(formKey);
        if (!repoPath) return;
        await taskStore.editTask({ repoPath, key: formKey, title: formTitle.trim(), description: formDescription, priority: formPriority, tags: null, blockedBy: null, baseBranch: formBaseBranch });
      }
      modalMode = null;
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

  // Flat navigation list
  type NavItem = { type: "task"; task: TaskItem; sectionKey: string; projectPath: string } | { type: "section"; sectionKey: string; label: string };

  const flatNav = $derived.by(() => {
    const result: NavItem[] = [];
    for (const project of projects) {
      const projectTasks = tasksByProject[project.path] ?? [];
      if (projectTasks.length === 0) continue;
      const statusGroups = groupByStatus(projectTasks);
      for (const status of statusOrder.filter(s => !(s === "done" && getSettings().hide_done_tasks))) {
        const sectionKey = `${project.path}:${status}`;
        const items = statusGroups[status] ?? [];
        if (items.length === 0) continue;
        if (collapsedSections[sectionKey]) {
          result.push({ type: "section", sectionKey, label: `${statusLabels[status] ?? status} (${items.length})` });
        } else {
          for (const t of items) result.push({ type: "task", task: t, sectionKey, projectPath: project.path });
        }
      }
    }
    return result;
  });

  const flatTaskKeys = $derived(flatNav.map((item) => item.type === "task" ? item.task.key : `§${item.sectionKey}`));

  // O(1) index lookup map (avoids O(n²) indexOf in template)
  const flatTaskIndex = $derived.by(() => {
    const map = new Map<string, number>();
    flatTaskKeys.forEach((key, i) => map.set(key, i));
    return map;
  });

  function handleTaskKeydown(e: KeyboardEvent) {
    if (disableKeyboard) return;
    if (getActiveZone() !== "sidebar") return;
    if (flatNav.length === 0) return;
    const el = document.activeElement;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.closest("[role='combobox']") || el.closest("[role='dialog']"))) return;

    clampIndex(flatNav.length);

    const current = flatNav[getSelectedIndex()];
    if (!current) return;

    // Fold section with left/h
    if (e.key === "ArrowLeft" || e.key === "h") {
      e.preventDefault();
      collapsedSections = { ...collapsedSections, [current.sectionKey]: true };
      return;
    }
    // Unfold section with right/l (only meaningful on collapsed section headers)
    if (e.key === "ArrowRight" || e.key === "l") {
      if (current.type === "section") {
        e.preventDefault();
        collapsedSections = { ...collapsedSections, [current.sectionKey]: false };
      }
      return;
    }

    const action = handleSidebarKey(e, flatNav.length);
    if (!action) return;

    // Enter/select on collapsed section header → unfold
    if (current.type === "section") {
      if (action.type === "select" || action.type === "start_session") {
        collapsedSections = { ...collapsedSections, [current.sectionKey]: false };
      }
      return;
    }

    const task = current.task;
    if (action.type === "select" || action.type === "start_session") {
      handleClick(task, current.projectPath);
    } else if (action.type === "status") {
      moveTask(task.key, action.status);
    } else if (action.type === "edit") {
      openEdit(task);
    } else if (action.type === "open_pr") {
      const linked = sessionForTask(task.key);
      if (linked?.pr_url) openUrl(linked.pr_url);
    }
  }
</script>

<svelte:window onkeydown={handleTaskKeydown} />

<div class="flex flex-col h-full">
  <!-- Task list: project > status -->
  <nav class="flex-1 overflow-y-auto px-2 py-2 space-y-3">
    {#if projects.length === 0}
      <p class="text-xs text-t2 text-center mt-8">No projects</p>
    {:else if Object.values(tasksByProject).every(t => t.length === 0) && !loading}
      <p class="text-xs text-t2 text-center mt-8">No tasks found</p>
    {:else}
      {#each projects as project (project.path)}
        {@const projectTasks = tasksByProject[project.path] ?? []}
        {#if projectTasks.length > 0}
          {@const statusGroups = groupByStatus(projectTasks)}
          <div>
            <h3 class="px-2 mb-1 text-[11px] font-semibold text-t2 uppercase tracking-wider truncate flex items-center gap-1">{project.name}</h3>
            {#each statusOrder.filter(s => !(s === "done" && getSettings().hide_done_tasks)) as status}
              {@const items = statusGroups[status] ?? []}
              {#if items.length > 0}
                {@const sectionKey = `${project.path}:${status}`}
                {@const sectionNavIdx = flatTaskIndex.get(`§${sectionKey}`) ?? -1}
                {@const isSectionSelected = getActiveZone() === 'sidebar' && sectionNavIdx === getSelectedIndex()}
                <div class="ml-1">
                  <button
                    class="w-full flex items-center gap-1.5 px-2 py-1 text-xs font-semibold {statusColors[status] ?? 'text-t3'} hover:opacity-80 rounded-md {isSectionSelected ? 'ring-1 ring-accent/50' : ''}"
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
                        {@const taskFlatIdx = flatTaskIndex.get(task.key) ?? -1}
                        {@const isTaskSelected = getActiveZone() === 'sidebar' && taskFlatIdx === getSelectedIndex()}
                        {@const isActive = task.key === activeTaskKey}
                        {@const isParent = isParentTask(task, projectTasks)}
                        <li>
                          <button
                            class="w-full text-left px-2 py-1.5 rounded-md text-sm flex items-center gap-1 transition-colors select-none
                              {isActive ? 'bg-accent-bg text-t1 font-medium' : 'text-t1 hover:bg-panel-hi'}
                              {isTaskSelected ? 'ring-1 ring-accent/50' : ''}"
                            onclick={() => handleClick(task, project.path)}
                            oncontextmenu={(e) => onContextMenuOpen(e, task)}
                          >
                            {#if task.parent_key}
                              <span class="shrink-0 text-[10px] text-t3 dark:text-t3">{task.parent_key} ›</span>
                            {/if}
                            <span class="shrink-0 text-[10px] font-medium {isParent ? 'text-t3' : 'text-accent'}">{task.key}</span>
                            <span class="truncate">{task.title}</span>
                            {#if sessionForTask(task.key)}
                              {@const linked = sessionForTask(task.key)!}
                              {#if agentStates[linked.id] === 'Busy'}
                                <span class="ml-auto shrink-0 size-3.5 animate-spin text-t3" title="Agent working">
                                  <LoaderCircle class="size-3.5" />
                                </span>
                              {:else if agentStates[linked.id] === 'Idle'}
                                <span class="ml-auto shrink-0 size-3.5 animate-pulse text-amber-500" title="Needs attention">
                                  <Lightbulb class="size-3.5" />
                                </span>
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
{#if modalMode !== null}
<div class="fixed inset-0 z-50 flex items-center justify-center" onkeydown={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label={modalMode === "create" ? "Create Task" : "Edit Task"}>
  <div class="w-[36rem] p-6 rounded-lg border border-border bg-panel shadow-lg">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div bind:this={taskFormWrapper} tabindex="-1" onkeydown={taskFk.handleKeydown} onfocusin={taskFk.handleFocusin} class="outline-none">
  <form
    class="space-y-4"
    onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}
    onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); handleSubmit(); } }}
  >
    <h2 class="text-lg font-semibold text-t1">{modalMode === "create" ? "Create Task" : "Edit Task"}</h2>

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

    <div class="space-y-1" data-field="title">
      <Label>Title <span class="font-mono text-[10px] px-1 rounded {taskFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">T</span></Label>
      <Input bind:value={formTitle} placeholder="Task title" />
    </div>

    <div class="space-y-1" data-field="desc">
      <Label>Description <span class="font-mono text-[10px] px-1 rounded {taskFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">D</span></Label>
      <textarea
        bind:value={formDescription}
        placeholder="Optional description"
        class="w-full rounded border border-border bg-panel-hi px-3 py-2 text-sm text-t1 placeholder:text-t3 resize-none min-h-[4rem] max-h-[50vh] overflow-y-auto focus:outline-none focus:ring-1 focus:ring-accent"
        rows="3"
        oninput={(e) => { const el = e.currentTarget; el.style.height = 'auto'; el.style.height = el.scrollHeight + 'px'; }}
        use:autoResize
      ></textarea>
    </div>

    <div class="space-y-1" data-field="priority">
      <Label>Priority <span class="font-mono text-[10px] px-1 rounded {taskFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">P</span></Label>
      <input type="number" bind:value={formPriority} class="w-20 rounded border border-border bg-panel-hi px-3 py-2 text-sm text-t1 focus:outline-none focus:ring-1 focus:ring-accent" />
    </div>

    <div class="space-y-1" data-field="base">
      <Label>Base branch <span class="font-mono text-[10px] px-1 rounded {taskFk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">B</span></Label>
      <Input bind:value={formBaseBranch} placeholder="main" />
    </div>

    <!-- Footer with mode indicator -->
    <div class="flex items-center justify-between pt-2 border-t border-border">
      <div class="flex items-center gap-2">
        {#if taskFk.mode === "insert"}
          <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-accent-bg text-accent font-medium">INSERT</span>
          <span class="text-[10px] text-t3">esc → normal mode</span>
        {:else}
          <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
          <span class="text-[10px] text-t3">press a key to focus field</span>
        {/if}
      </div>
      <div class="flex gap-2">
        <Button type="button" onclick={() => { modalMode = null; focusTerminal(); }}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!formTitle.trim()}>{modalMode === "create" ? "Create" : "Save"} <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span></Button>
      </div>
    </div>
  </form>
  </div>
</div>
</div>
{/if}
