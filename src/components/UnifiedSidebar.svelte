<script lang="ts">
  import { tasks as tasksApi } from "../lib/api";
  import { projects as projectsApi } from "../lib/api";
  import type { TaskItem, Session, Project } from "../lib/types";
  import { focusTerminal, getActiveZone } from "../lib/focus.svelte";
  import type { FocusZone } from "../lib/focus.svelte";
  import { getSelectedIndex, setSelectedIndex, clampIndex, handleSidebarKey } from "../lib/sidebar-nav.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ChevronDown, ChevronRight, Lightbulb, LoaderCircle, Zap, GitFork, GitPullRequest, GitMerge, Plus, Settings } from "@lucide/svelte";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { MOD_LABEL } from "../lib/keyboard";
  import TaskPanel from "./TaskPanel.svelte";

  interface Props {
    projects: Project[];
    sessions: Session[];
    activeSessionId: string | null;
    zone: FocusZone;
    agentStates: Record<string, string>;
    renamingSessionId: string | null;
    taskCreateRequested?: boolean;
    taskRefreshRequested?: boolean;
    onAddProject: () => void;
    onSelectSession: (id: string) => void;
    onArchiveSession: (session: Session) => void;
    onDeleteSession: (session: Session) => void;
    onRestartSession: (session: Session) => void;
    onOpenPreferences: () => void;
    onRenameSession: (id: string, name: string) => void;
    onStartRename: (id: string) => void;
    onArchiveProject: (project: Project) => void;
    onDeleteProject: (project: Project) => void;
    onPickTask: (task: TaskItem, repoPath: string) => void;
    onCreateSession?: () => void;
    onTaskCreateConsumed?: () => void;
    onTaskRefreshConsumed?: () => void;
    onSessionsChanged?: () => void;
  }

  let { projects, sessions, activeSessionId, zone, agentStates, renamingSessionId, taskCreateRequested = false, taskRefreshRequested = false, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onRestartSession, onOpenPreferences, onRenameSession, onStartRename, onArchiveProject, onDeleteProject, onPickTask, onCreateSession, onTaskCreateConsumed, onTaskRefreshConsumed, onSessionsChanged }: Props = $props();

  let sidebarWidth = $state(getLayoutWidth("sidebar", 224));
  let tasksByProject = $state<Record<string, TaskItem[]>>({});
  let collapsedSections = $state<Record<string, boolean>>({ done: true });
  let renameValue = $state("");

  const statusOrder = ["in_progress", "in_review", "todo", "done"];
  const statusLabels: Record<string, string> = { in_progress: "In Progress", in_review: "In Review", todo: "Todo", done: "Done" };
  const statusColors: Record<string, string> = { todo: "text-blue-500 dark:text-blue-400", in_progress: "text-amber-500 dark:text-amber-400", in_review: "text-green-500 dark:text-green-400", done: "text-purple-500 dark:text-purple-400" };

  // Auto-mode per project
  let projectAutoMode = $state<Record<string, boolean>>({});
  async function loadAutoModes() {
    for (const p of projects) {
      try { projectAutoMode[p.id] = await projectsApi.getAutoMode(p.id); } catch { /* ignore */ }
    }
  }
  $effect(() => { if (projects.length) loadAutoModes(); });
  async function toggleAutoMode(project: Project) {
    const current = projectAutoMode[project.id] ?? false;
    await projectsApi.setAutoMode(project.id, !current);
    projectAutoMode[project.id] = !current;
  }

  // Rename
  $effect(() => {
    if (renamingSessionId) {
      const s = sessions.find((x) => x.id === renamingSessionId);
      if (s) renameValue = s.name || s.branch;
    }
  });
  function autofocus(node: HTMLInputElement) { requestAnimationFrame(() => node.focus()); }
  function startRename(session: Session) {
    renameValue = session.name || session.branch;
    onStartRename(session.id);
  }
  function commitRename(id: string) {
    const trimmed = renameValue.trim();
    if (trimmed) onRenameSession(id, trimmed);
    onStartRename("");
  }

  // Context menus
  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);
  let projectContextMenu = $state<{ x: number; y: number; project: Project } | null>(null);
  let taskContextMenu = $state<{ x: number; y: number; task: TaskItem; projectPath: string } | null>(null);

  function onContextMenu(e: MouseEvent, session: Session) { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, session }; }
  function onProjectContextMenu(e: MouseEvent, project: Project) { e.preventDefault(); projectContextMenu = { x: e.clientX, y: e.clientY, project }; }
  function onTaskContextMenu(e: MouseEvent, task: TaskItem, projectPath: string) { e.preventDefault(); taskContextMenu = { x: e.clientX, y: e.clientY, task, projectPath }; }

  // Load tasks
  export async function refresh() {
    if (projects.length === 0) return;
    const results: Record<string, TaskItem[]> = {};
    await Promise.all(projects.map(async (p) => {
      try { results[p.path] = await tasksApi.listAll(p.path); } catch { results[p.path] = []; }
    }));
    tasksByProject = results;
  }
  $effect(() => { if (projects.length > 0) refresh(); });

  // External triggers
  let taskPanelRef = $state<TaskPanel | undefined>(undefined);
  $effect(() => { if (taskCreateRequested) { taskPanelRef?.openCreate(); onTaskCreateConsumed?.(); } });
  $effect(() => { if (taskRefreshRequested) { refresh(); onTaskRefreshConsumed?.(); } });

  // Derive orphan sessions (no task_key or task_key not in loaded tasks)
  const allTaskKeys = $derived(new Set(Object.values(tasksByProject).flat().map(t => t.key)));
  const orphanSessions = $derived(sessions.filter(s => !s.task_key || !allTaskKeys.has(s.task_key)));
  const orphansByProject = $derived(
    projects.map(p => ({ project: p, sessions: orphanSessions.filter(s => s.project_id === p.id) })).filter(g => g.sessions.length > 0)
  );

  function sessionForTask(key: string): Session | undefined {
    return sessions.find(s => s.task_key === key);
  }

  function groupByStatus(items: TaskItem[]): Record<string, TaskItem[]> {
    const groups: Record<string, TaskItem[]> = {};
    for (const s of statusOrder) groups[s] = [];
    for (const t of items) (groups[t.status] ?? (groups["todo"] ??= [])).push(t);
    for (const s of statusOrder) groups[s]?.sort((a, b) => b.priority - a.priority);
    return groups;
  }

  function toggleSection(key: string) {
    collapsedSections = { ...collapsedSections, [key]: !collapsedSections[key] };
  }

  function handleTaskClick(task: TaskItem, projectPath: string) {
    const linked = sessionForTask(task.key);
    if (linked) { onSelectSession(linked.id); focusTerminal(); }
    else onPickTask(task, projectPath);
  }

  function handleOrphanClick(session: Session) {
    onSelectSession(session.id);
    focusTerminal();
  }

  async function moveTask(key: string, status: string) {
    const repoPath = repoPathForTask(key);
    if (!repoPath) return;
    await tasksApi.move(key, status, repoPath);
    await refresh();
    onSessionsChanged?.();
  }

  function repoPathForTask(key: string): string | null {
    for (const [path, items] of Object.entries(tasksByProject)) {
      if (items.some(t => t.key === key)) return path;
    }
    return projects[0]?.path ?? null;
  }

  // Flat nav list for keyboard navigation
  type NavItem = { type: "orphan"; session: Session } | { type: "task"; task: TaskItem; projectPath: string };
  const flatNav = $derived.by(() => {
    const result: NavItem[] = [];
    for (const project of projects) {
      // Orphans first
      const projectOrphans = orphansByProject.find(g => g.project.id === project.id)?.sessions ?? [];
      for (const s of projectOrphans) result.push({ type: "orphan", session: s });
      // Then tasks by status
      const projectTasks = tasksByProject[project.path] ?? [];
      const statusGroups = groupByStatus(projectTasks);
      for (const status of statusOrder.filter(s => !(s === "done" && getSettings().hide_done_tasks))) {
        const sectionKey = `${project.path}:${status}`;
        if (collapsedSections[sectionKey]) continue;
        for (const t of (statusGroups[status] ?? [])) result.push({ type: "task", task: t, projectPath: project.path });
      }
    }
    return result;
  });

  $effect(() => { clampIndex(flatNav.length); });

  function handleKeydown(e: KeyboardEvent) {
    if (zone !== "sidebar") return;
    if (flatNav.length === 0) return;
    const el = document.activeElement;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.closest("[role='combobox']") || el.closest("[role='dialog']"))) return;

    const action = handleSidebarKey(e, flatNav.length);
    if (!action) return;

    const current = flatNav[getSelectedIndex()];
    if (!current) return;

    if (current.type === "orphan") {
      const session = current.session;
      if (action.type === "select") { onSelectSession(session.id); focusTerminal(); }
      else if (action.type === "archive") onArchiveSession(session);
      else if (action.type === "delete") onDeleteSession(session);
      else if (action.type === "rename") startRename(session);
      else if (action.type === "restart") onRestartSession(session);
      else if (action.type === "open_pr") { if (session.pr_url) openUrl(session.pr_url); }
    } else {
      const task = current.task;
      if (action.type === "select" || action.type === "start_session") handleTaskClick(task, current.projectPath);
      else if (action.type === "status") moveTask(task.key, action.status);
      else if (action.type === "open_pr") { const linked = sessionForTask(task.key); if (linked?.pr_url) openUrl(linked.pr_url); }
    }
  }

  // Window focus refresh
  function onWindowFocus() { refresh(); }
</script>

<svelte:window onkeydown={handleKeydown} onfocus={onWindowFocus} />

<aside class="relative shrink-0 flex flex-col border-r border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-950 {zone === 'sidebar' ? 'ring-1 ring-inset ring-primary-500/30' : ''}" style:width="{sidebarWidth}px">
  <ResizeHandle side="right" bind:width={sidebarWidth} min={160} max={Infinity} defaultWidth={224} onResizeEnd={(w) => setLayoutWidth("sidebar", w)} />

  <!-- Header -->
  <div class="flex items-center justify-between px-3 py-2 border-b border-surface-200 dark:border-surface-800">
    <span class="text-xs font-semibold text-surface-600 dark:text-surface-400 uppercase tracking-wider">Workspace</span>
    <button
      onclick={() => onCreateSession?.()}
      title="New… ({MOD_LABEL}N)"
      class="size-6 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors"
    >
      <Plus class="size-4" />
    </button>
  </div>

  <!-- Main content -->
  <nav class="flex-1 overflow-y-auto px-2 py-2 space-y-3">
    {#if projects.length === 0}
      <div class="mt-12 text-center px-4 space-y-3">
        <p class="text-xs text-surface-600 dark:text-surface-400">No projects yet</p>
        <button onclick={onAddProject} class="text-xs text-primary-600 dark:text-primary-400 hover:underline">Add a project →</button>
      </div>
    {:else}
      {#each projects as project (project.id)}
        {@const projectTasks = tasksByProject[project.path] ?? []}
        {@const statusGroups = groupByStatus(projectTasks)}
        {@const projectOrphans = orphansByProject.find(g => g.project.id === project.id)?.sessions ?? []}
        <div>
          <h3
            class="px-2 mb-1 text-[11px] font-semibold text-surface-600 dark:text-surface-400 uppercase tracking-wider truncate flex items-center gap-1"
            title={project.path}
            oncontextmenu={(e) => onProjectContextMenu(e, project)}
          >
            {project.name}
            {#if projectAutoMode[project.id]}<Zap class="size-2.5 text-amber-500" />{/if}
          </h3>

          <!-- Orphan sessions at top -->
          {#if projectOrphans.length > 0}
            <ul class="space-y-0.5 ml-1 mb-1">
              {#each projectOrphans as session (session.id)}
                {@const globalIndex = flatNav.findIndex(n => n.type === "orphan" && n.session.id === session.id)}
                {@const isActive = session.id === activeSessionId}
                {@const isSelected = zone === 'sidebar' && globalIndex === getSelectedIndex()}
                <li>
                  {#if renamingSessionId === session.id}
                    <input
                      use:autofocus
                      class="w-full px-2 py-1.5 rounded-md text-sm bg-surface-50 dark:bg-surface-800 border border-primary-500 outline-none"
                      bind:value={renameValue}
                      onkeydown={(e) => { if (e.key === 'Enter') commitRename(session.id); if (e.key === 'Escape') onStartRename(""); }}
                      onblur={() => commitRename(session.id)}
                    />
                  {:else}
                    <button
                      class="w-full text-left px-2 py-1.5 rounded-md text-sm flex items-center gap-1 transition-colors
                        {isActive ? 'bg-primary-500/15 text-primary-700 dark:text-surface-50 font-medium' : 'text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800'}
                        {isSelected ? 'ring-1 ring-primary-500/50' : ''}
                        {session.status === 'exited' ? 'opacity-60' : ''}"
                      onclick={() => handleOrphanClick(session)}
                      oncontextmenu={(e) => onContextMenu(e, session)}
                    >
                      {#if session.worktree_path}<GitFork class="size-3 shrink-0 text-surface-600 dark:text-surface-400" />{/if}
                      <span class="truncate">{session.name || session.branch}</span>
                      <span class="ml-auto shrink-0 flex items-center gap-1">
                        {#if session.pr_url}
                          <button
                            class="shrink-0 size-3.5 {session.pr_state === 'merged' ? 'text-purple-600 dark:text-purple-400' : session.pr_state === 'draft' ? 'text-surface-500 dark:text-surface-400' : 'text-green-600 dark:text-green-400'}"
                            title="Open PR ({session.pr_state})"
                            tabindex="-1"
                            onmousedown={(e) => e.preventDefault()}
                            onclick={(e) => { e.stopPropagation(); openUrl(session.pr_url!); }}
                          >
                            {#if session.pr_state === "merged"}<GitMerge class="size-3.5" />{:else}<GitPullRequest class="size-3.5" />{/if}
                          </button>
                        {/if}
                        {#if session.status === 'exited'}
                          <span class="text-[10px] font-medium text-surface-500 bg-surface-200 dark:bg-surface-800 rounded px-1">exited</span>
                        {:else if agentStates[session.id] === 'Busy'}
                          <LoaderCircle class="size-3.5 animate-spin text-surface-500" />
                        {:else if agentStates[session.id] === 'Idle'}
                          <Lightbulb class="size-3.5 animate-pulse text-amber-500" />
                        {/if}
                      </span>
                    </button>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}

          <!-- Tasks grouped by status -->
          {#each statusOrder.filter(s => !(s === "done" && getSettings().hide_done_tasks)) as status}
            {@const items = statusGroups[status] ?? []}
            {#if items.length > 0}
              {@const sectionKey = `${project.path}:${status}`}
              <div class="ml-1">
                <button
                  class="w-full flex items-center gap-1.5 px-2 py-1 text-xs font-semibold {statusColors[status]} hover:opacity-80 rounded-md"
                  onclick={() => toggleSection(sectionKey)}
                >
                  {#if collapsedSections[sectionKey]}<ChevronRight class="size-3" />{:else}<ChevronDown class="size-3" />{/if}
                  {statusLabels[status]}
                  <span class="font-normal ml-0.5">({items.length})</span>
                </button>
                {#if !collapsedSections[sectionKey]}
                  <ul class="space-y-0.5 ml-1">
                    {#each items as task (task.key)}
                      {@const linked = sessionForTask(task.key)}
                      {@const isActive = linked?.id === activeSessionId}
                      {@const taskNavIdx = flatNav.findIndex(n => n.type === "task" && n.task.key === task.key)}
                      {@const isSelected = zone === 'sidebar' && taskNavIdx === getSelectedIndex()}
                      <li>
                        <button
                          class="w-full text-left px-2 py-1.5 rounded-md text-sm flex items-center gap-1 transition-colors
                            {isActive ? 'bg-primary-500/15 text-primary-700 dark:text-surface-50 font-medium' : 'text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800'}
                            {isSelected ? 'ring-1 ring-primary-500/50' : ''}"
                          onclick={() => handleTaskClick(task, project.path)}
                          oncontextmenu={(e) => onTaskContextMenu(e, task, project.path)}
                        >
                          <span class="shrink-0 text-[10px] font-medium text-primary-600 dark:text-primary-400">{task.key}</span>
                          <span class="truncate">{task.title}</span>
                          {#if linked}
                            <span class="ml-auto shrink-0 flex items-center gap-1">
                              {#if agentStates[linked.id] === 'Busy'}
                                <LoaderCircle class="size-3.5 animate-spin text-surface-500" />
                              {:else if agentStates[linked.id] === 'Idle'}
                                <Lightbulb class="size-3.5 animate-pulse text-amber-500" />
                              {/if}
                              {#if linked.pr_url}
                                <button
                                  class="shrink-0 size-3.5 {linked.pr_state === 'merged' ? 'text-purple-600 dark:text-purple-400' : 'text-green-600 dark:text-green-400'}"
                                  tabindex="-1"
                                  onmousedown={(e) => e.preventDefault()}
                                  onclick={(e) => { e.stopPropagation(); openUrl(linked.pr_url!); }}
                                >
                                  {#if linked.pr_state === "merged"}<GitMerge class="size-3.5" />{:else}<GitPullRequest class="size-3.5" />{/if}
                                </button>
                              {/if}
                            </span>
                          {/if}
                        </button>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
          {/each}

          {#if projectOrphans.length === 0 && projectTasks.length === 0}
            <p class="px-3 py-1 text-xs text-surface-600 dark:text-surface-400 italic">No sessions or tasks</p>
          {/if}
        </div>
      {/each}
    {/if}
  </nav>

  <!-- Settings -->
  <div class="px-3 py-2 border-t border-surface-200 dark:border-surface-800">
    <button onclick={onOpenPreferences} title="Preferences ({MOD_LABEL},)" class="size-7 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors">
      <Settings class="size-4" />
    </button>
  </div>
</aside>

<!-- Hidden TaskPanel for create dialog -->
<div class="hidden">
  <TaskPanel
    bind:this={taskPanelRef}
    disableKeyboard={true}
    projects={projects.map(p => ({ name: p.name, path: p.path }))}
    projectAutoMode={Object.fromEntries(projects.map(p => [p.path, projectAutoMode[p.id] ?? false]))}
    sessions={sessions.map(s => ({ id: s.id, task_key: s.task_key, pr_url: s.pr_url }))}
    {activeSessionId}
    {agentStates}
    {taskCreateRequested}
    taskRefreshRequested={false}
    {onPickTask}
    {onSelectSession}
    onArchiveSession={async (s) => { const full = sessions.find(x => x.id === s.id); if (full) await onArchiveSession(full); }}
    onTaskCreateConsumed={onTaskCreateConsumed}
    onTaskRefreshConsumed={() => {}}
    onSessionsChanged={onSessionsChanged}
  />
</div>

<!-- Session context menu -->
{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={contextMenu.session.status === 'exited'
      ? [
          { label: "Restart", onSelect: () => onRestartSession(contextMenu!.session) },
          { label: "Delete", danger: true, onSelect: () => onDeleteSession(contextMenu!.session) },
        ]
      : [
          { label: "Rename", onSelect: () => startRename(contextMenu!.session) },
          { label: "Archive", onSelect: () => onArchiveSession(contextMenu!.session) },
          { label: "Delete", danger: true, onSelect: () => onDeleteSession(contextMenu!.session) },
        ]}
  />
{/if}

<!-- Project context menu -->
{#if projectContextMenu}
  <ContextMenu
    x={projectContextMenu.x}
    y={projectContextMenu.y}
    onClose={() => (projectContextMenu = null)}
    items={[
      { label: projectAutoMode[projectContextMenu.project.id] ? "✓ Auto-dispatch" : "Auto-dispatch", onSelect: () => toggleAutoMode(projectContextMenu!.project) },
      { label: "Archive project", onSelect: () => onArchiveProject(projectContextMenu!.project) },
      { label: "Delete project", danger: true, onSelect: () => onDeleteProject(projectContextMenu!.project) },
    ]}
  />
{/if}

<!-- Task context menu -->
{#if taskContextMenu}
  {@const linkedSession = sessionForTask(taskContextMenu.task.key)}
  <ContextMenu
    x={taskContextMenu.x}
    y={taskContextMenu.y}
    onClose={() => (taskContextMenu = null)}
    items={[
      ...(linkedSession
        ? [{ label: "Go to session", onSelect: () => { onSelectSession(linkedSession.id); focusTerminal(); } }]
        : [{ label: "Start session", onSelect: () => onPickTask(taskContextMenu!.task, taskContextMenu!.projectPath) }]),
      ...(taskContextMenu.task.status !== "in_progress" ? [{ label: "→ In Progress", onSelect: () => moveTask(taskContextMenu!.task.key, "in_progress") }] : []),
      ...(taskContextMenu.task.status !== "in_review" ? [{ label: "→ In Review", onSelect: () => moveTask(taskContextMenu!.task.key, "in_review") }] : []),
      ...(taskContextMenu.task.status !== "todo" ? [{ label: "→ Todo", onSelect: () => moveTask(taskContextMenu!.task.key, "todo") }] : []),
      ...(taskContextMenu.task.status !== "done" ? [{ label: "→ Done", onSelect: () => moveTask(taskContextMenu!.task.key, "done") }] : []),
    ]}
  />
{/if}
