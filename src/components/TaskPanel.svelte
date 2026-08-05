<script lang="ts">
  import type { TaskItem, Session, Project } from "../lib/types";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { focusTerminal, getActiveZone } from "../lib/focus.svelte";
  import { getSelectedIndex, setSelectedIndex, clampIndex, handleSidebarKey } from "../lib/sidebar-nav.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { Button, ContextMenu } from "./ui";
  import TaskForm from "./TaskForm.svelte";
  import { ChevronDown, ChevronRight, Lightbulb, LoaderCircle } from "@lucide/svelte";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    onPickTask: (task: TaskItem, repoPath: string) => void;
    onSelectSession: (id: string) => void;
    onArchiveSession?: (session: Pick<Session, "id" | "task_key" | "pr_url">) => void | Promise<void>;
    onSessionsChanged?: () => void;
    onSessionCreated?: (session: Session) => void;
    disableKeyboard?: boolean;
  }

  let { disableKeyboard = false, onPickTask, onSelectSession, onArchiveSession, onSessionsChanged, onSessionCreated }: Props = $props();

  // ─── Derived from stores ────────────────────────────────────────────────────
  const projects = $derived(projectStore.getProjects().map(p => ({ id: p.id, name: p.name, path: p.path })));
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
  let editingTask = $state<TaskItem | null>(null);

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
    editingTask = null;
    modalMode = "create";
  }

  export function openEdit(task: TaskItem) {
    editingTask = task;
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
  <div class="w-[36rem] max-h-[85vh] flex flex-col p-6 rounded-lg border border-border bg-panel shadow-lg overflow-hidden">
    <h2 class="flex-shrink-0 text-lg font-semibold text-t1 px-5 pb-2">{modalMode === "create" ? "Create Task" : "Edit Task"}</h2>
    <div class="flex-1 min-h-0 overflow-y-auto">
      <TaskForm
        mode={modalMode}
        projects={projects.map(p => ({ id: p.id, name: p.name, path: p.path }))}
        tasks={Object.values(tasksByProject).flat()}
        sessions={sessions}
        initial={modalMode === "edit" && editingTask ? {
          key: editingTask.key,
          title: editingTask.title,
          description: editingTask.description,
          priority: editingTask.priority,
          parentKey: editingTask.parent_key,
          blockedBy: editingTask.blocked_by,
          tags: editingTask.tags,
          baseBranch: editingTask.base_branch,
          projectPath: repoPathForTask(editingTask.key) ?? projects[0]?.path ?? "",
        } : { projectPath: projects[0]?.path ?? "" }}
        onSubmitted={() => { modalMode = null; focusTerminal(); }}
        onCancel={() => { modalMode = null; focusTerminal(); }}
        onSessionCreated={(session) => { onSessionCreated?.(session); }}
      />
    </div>
  </div>
</div>
{/if}
