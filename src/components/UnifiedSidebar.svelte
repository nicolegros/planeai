<script lang="ts">
  import { projects as projectsApi } from "../lib/api";
  import type { TaskItem, Session, Project } from "../lib/types";
  import { focusTerminal, getActiveZone, getSidebarSubZone } from "../lib/focus.svelte";
  import { getSelectedIndex, setSelectedIndex, clampIndex, handleSidebarKey } from "../lib/sidebar-nav.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ChevronDown, ChevronRight, LoaderCircle, Zap, Plus, CheckCircle2, XCircle, Lightbulb, Settings } from "@lucide/svelte";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { MOD_LABEL } from "../lib/keyboard";
  import { getPreviewId } from "../lib/session-nav-cycle.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import { getCiStatus } from "../lib/ci-checks.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    renamingSessionId: string | null;
    taskCreateRequested?: boolean;
    onAddProject: () => void;
    onSelectSession: (id: string) => void;
    onArchiveSession: (session: Session) => void;
    onDeleteSession: (session: Session) => void;
    onRestartSession: (session: Session) => void;
    onOpenPreferences: () => void;
    onRenameSession: (id: string, name: string) => void;
    onStartRename: (id: string) => void;
    onDeleteProject: (project: Project) => void;
    onPickTask: (task: TaskItem, repoPath: string) => void;
    onCreateSession?: () => void;
    onTaskCreateConsumed?: () => void;
    onSessionsChanged?: () => void;
  }

  let { renamingSessionId, taskCreateRequested = false, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onRestartSession, onOpenPreferences, onRenameSession, onStartRename, onDeleteProject, onPickTask, onCreateSession, onTaskCreateConsumed, onSessionsChanged }: Props = $props();

  // ─── Derived from stores ────────────────────────────────────────────────────
  const projects = $derived(projectStore.getProjects());
  const sessions = $derived(orchestrator.getSessions());
  const activeSessionId = $derived(orchestrator.getActiveSessionId());
  const agentStates = $derived(orchestrator.getAgentStates());
  const zone = $derived(getActiveZone());
  const tasksByProject = $derived(taskStore.getTasksByProject());

  let sidebarWidth = $state(getLayoutWidth("sidebar", 266));
  let collapsedSections = $state<Record<string, boolean>>({ done: true });
  let renameValue = $state("");
  let searchQuery = $state("");

  const statusOrder = ["running", "needs_review", "idle", "todo", "done", "exited"];
  const statusLabels: Record<string, string> = { running: "Running", needs_review: "Needs review", idle: "Idle", todo: "To do", done: "Done", exited: "Exited" };
  const statusDotColors: Record<string, string> = { running: "bg-status-running", needs_review: "bg-status-review", idle: "bg-status-idle", todo: "bg-t3", done: "bg-status-running", exited: "bg-status-exited" };

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

  // Empty projects default to collapsed unless explicitly toggled
  function isProjectCollapsed(project: Project): boolean {
    const key = `project:${project.id}`;
    return collapsedSections[key] ?? (
      (orphansByProject.find(g => g.project.id === project.id)?.sessions ?? []).length === 0 &&
      (tasksByProject[project.path] ?? []).length === 0
    );
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

  // External triggers
  let taskPanelRef = $state<TaskPanel | undefined>(undefined);
  $effect(() => { if (taskCreateRequested) { taskPanelRef?.openCreate(); onTaskCreateConsumed?.(); } });

  // Derive orphan sessions (no task_key or task_key not in loaded tasks)
  const allTaskKeys = $derived(new Set(Object.values(tasksByProject).flat().map(t => t.key)));
  const orphanSessions = $derived(sessions.filter(s => !s.task_key || !allTaskKeys.has(s.task_key)));
  const orphansByProject = $derived(
    projects.map(p => ({ project: p, sessions: orphanSessions.filter(s => s.project_id === p.id) })).filter(g => g.sessions.length > 0)
  );

  const previewSessionId = $derived(getPreviewId());

  function sessionForTask(key: string): Session | undefined {
    return sessions.find(s => s.task_key === key);
  }

  function isParentTask(task: TaskItem, allTasks: TaskItem[]): boolean {
    return allTasks.some(t => t.parent_key === task.key);
  }

  function groupByStatus(items: TaskItem[]): Record<string, TaskItem[]> {
    const groups: Record<string, TaskItem[]> = {};
    for (const s of statusOrder) groups[s] = [];
    for (const t of items) {
      const bucket = t.status === "in_progress" ? "running" : t.status === "in_review" ? "needs_review" : t.status === "done" ? "done" : "todo";
      (groups[bucket] ?? (groups["todo"] ??= [])).push(t);
    }
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
    await taskStore.moveTask(key, status, repoPath);
    onSessionsChanged?.();
  }

  function repoPathForTask(key: string): string | null {
    for (const [path, items] of Object.entries(tasksByProject)) {
      if (items.some(t => t.key === key)) return path;
    }
    return projects[0]?.path ?? null;
  }

  // Flat nav list for keyboard navigation
  type NavItem = { type: "project_header"; project: Project } | { type: "orphan"; session: Session } | { type: "status_header"; projectPath: string; status: string } | { type: "task"; task: TaskItem; projectPath: string };
  const flatNav = $derived.by(() => {
    const result: NavItem[] = [];
    for (const project of projects) {
      const projectKey = `project:${project.id}`;
      result.push({ type: "project_header", project });
      if (isProjectCollapsed(project)) continue;
      // Orphans first
      const projectOrphans = orphansByProject.find(g => g.project.id === project.id)?.sessions ?? [];
      for (const s of projectOrphans) result.push({ type: "orphan", session: s });
      // Then tasks by status
      const projectTasks = tasksByProject[project.path] ?? [];
      const statusGroups = groupByStatus(projectTasks);
      for (const status of statusOrder.filter(s => !(s === "done" && getSettings().hide_done_tasks))) {
        if ((statusGroups[status] ?? []).length === 0) continue;
        const sectionKey = `${project.path}:${status}`;
        result.push({ type: "status_header", projectPath: project.path, status });
        if (collapsedSections[sectionKey]) continue;
        for (const t of (statusGroups[status] ?? [])) result.push({ type: "task", task: t, projectPath: project.path });
      }
    }
    return result;
  });

  // O(1) lookup map for nav index (avoids O(n²) findIndex in template)
  const flatNavIndex = $derived.by(() => {
    const map = new Map<string, number>();
    flatNav.forEach((item, i) => {
      if (item.type === "project_header") map.set(`project:${item.project.id}`, i);
      else if (item.type === "orphan") map.set(`orphan:${item.session.id}`, i);
      else if (item.type === "status_header") map.set(`status:${item.projectPath}:${item.status}`, i);
      else if (item.type === "task") map.set(`task:${item.task.key}`, i);
    });
    return map;
  });

  $effect(() => { clampIndex(flatNav.length); });

  // Auto-focus active session when sessions panel is toggled
  $effect(() => {
    if (zone !== "sidebar" || getSidebarSubZone() !== "sessions") return;
    if (!activeSessionId) return;
    // Try orphan lookup first
    let idx = flatNavIndex.get(`orphan:${activeSessionId}`);
    // If not found, look up via task_key
    if (idx === undefined) {
      const active = sessions.find(s => s.id === activeSessionId);
      if (active?.task_key) idx = flatNavIndex.get(`task:${active.task_key}`);
    }
    if (idx !== undefined) setSelectedIndex(idx);
  });

  function handleKeydown(e: KeyboardEvent) {
    if (zone !== "sidebar") return;
    if (flatNav.length === 0) return;
    const el = document.activeElement;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.closest("[role='combobox']") || el.closest("[role='dialog']"))) return;

    const action = handleSidebarKey(e, flatNav.length);
    if (!action) return;

    const idx = getSelectedIndex();
    const current = flatNav[idx];
    if (!current) return;

    if (action.type === "collapse") {
      if (current.type === "project_header") {
        const key = `project:${current.project.id}`;
        if (!collapsedSections[key]) collapsedSections = { ...collapsedSections, [key]: true };
      } else if (current.type === "status_header") {
        const sectionKey = `${current.projectPath}:${current.status}`;
        if (!collapsedSections[sectionKey]) {
          collapsedSections = { ...collapsedSections, [sectionKey]: true };
        } else {
          // Already collapsed status → jump to project header
          const projectIdx = flatNav.findIndex(n => n.type === "project_header" && n.project.path === current.projectPath);
          if (projectIdx >= 0) setSelectedIndex(projectIdx);
        }
      } else if (current.type === "task" || current.type === "orphan") {
        // Jump to parent status header or project header
        for (let i = idx - 1; i >= 0; i--) {
          if (flatNav[i].type === "status_header" || flatNav[i].type === "project_header") {
            setSelectedIndex(i);
            break;
          }
        }
      }
      return;
    }

    if (action.type === "expand") {
      if (current.type === "project_header") {
        const key = `project:${current.project.id}`;
        if (collapsedSections[key]) collapsedSections = { ...collapsedSections, [key]: false };
      } else if (current.type === "status_header") {
        const sectionKey = `${current.projectPath}:${current.status}`;
        if (collapsedSections[sectionKey]) collapsedSections = { ...collapsedSections, [sectionKey]: false };
      }
      return;
    }

    if (current.type === "project_header") {
      if (action.type === "select") {
        const key = `project:${current.project.id}`;
        toggleSection(key);
      }
      return;
    }

    if (current.type === "status_header") {
      if (action.type === "select") {
        const sectionKey = `${current.projectPath}:${current.status}`;
        toggleSection(sectionKey);
      }
      return;
    }

    if (current.type === "orphan") {
      const session = current.session;
      if (action.type === "select") { onSelectSession(session.id); focusTerminal(); }
      else if (action.type === "archive") onArchiveSession(session);
      else if (action.type === "delete") onDeleteSession(session);
      else if (action.type === "rename") startRename(session);
      else if (action.type === "restart") onRestartSession(session);
      else if (action.type === "open_pr") { if (session.pr_url) openUrl(session.pr_url); }
      else if (action.type === "review") { onSelectSession(session.id); orchestrator.toggleDiff(); }
    } else {
      const task = current.task;
      if (action.type === "select" || action.type === "start_session") handleTaskClick(task, current.projectPath);
      else if (action.type === "edit") taskPanelRef?.openEdit(task);
      else if (action.type === "status") moveTask(task.key, action.status);
      else if (action.type === "open_pr") { const linked = sessionForTask(task.key); if (linked?.pr_url) openUrl(linked.pr_url); }
      else if (action.type === "review") { const linked = sessionForTask(task.key); if (linked) { onSelectSession(linked.id); orchestrator.toggleDiff(); } }
    }
  }

  // Window focus refresh (debounced to avoid IPC storms from rapid alt-tabbing)
  let lastFocusRefresh = 0;
  const FOCUS_REFRESH_COOLDOWN_MS = 5000;
  function onWindowFocus() {
    const now = Date.now();
    if (now - lastFocusRefresh < FOCUS_REFRESH_COOLDOWN_MS) return;
    lastFocusRefresh = now;
    taskStore.refresh(projects.map(p => p.path));
  }
</script>

<svelte:window onkeydown={handleKeydown} onfocus={onWindowFocus} />

{#snippet ciBadge(id: string)}
  {@const ci = getCiStatus(id)}
  {#if ci === 'passing'}
    <CheckCircle2 class="size-3 text-status-running" title="CI passing" />
  {:else if ci === 'failing'}
    <XCircle class="size-3 text-status-exited" title="CI failing" />
  {:else if ci === 'running'}
    <LoaderCircle class="size-3 animate-spin text-t3" title="CI running" />
  {/if}
{/snippet}

<aside class="relative shrink-0 flex flex-col border-r border-border bg-sidebar" style:width="{sidebarWidth}px" style:box-shadow="{zone === 'sidebar' ? 'inset 2px 0 0 0 var(--theme-accent)' : 'none'}">
  <ResizeHandle side="right" bind:width={sidebarWidth} min={160} max={Infinity} defaultWidth={266} onResizeEnd={(w) => setLayoutWidth("sidebar", w)} />

  <!-- Header: new session button -->
  <div class="px-3 pt-3 pb-2">
    <button
      onclick={() => onCreateSession?.()}
      class="w-full h-[32px] flex items-center justify-between px-3 rounded-lg bg-panel-hi border border-border text-t2 text-[12px] font-medium hover:opacity-80 transition-opacity"
    >
      <span class="flex items-center gap-1.5">
        <Plus class="size-3.5" />
        New session
      </span>
      <span class="font-mono text-[10px] text-t3">{MOD_LABEL}N</span>
    </button>
  </div>

  <!-- Main content -->
  <nav class="flex-1 overflow-y-auto overflow-x-hidden px-2 py-2 space-y-3 scrollbar-hide">
    {#if projects.length === 0}
      <div class="mt-12 text-center px-4 space-y-3">
        <p class="text-xs text-t3">No projects yet</p>
        <button onclick={onAddProject} class="text-xs text-accent hover:underline">Add a project →</button>
      </div>
    {:else}
      {#each projects as project (project.id)}
        {@const projectTasks = tasksByProject[project.path] ?? []}
        {@const statusGroups = groupByStatus(projectTasks)}
        {@const projectOrphans = orphansByProject.find(g => g.project.id === project.id)?.sessions ?? []}
        {@const projectKey = `project:${project.id}`}
        {@const projectCollapsed = isProjectCollapsed(project)}
        {@const projectNavIdx = flatNavIndex.get(`project:${project.id}`) ?? -1}
        {@const isProjectSelected = zone === 'sidebar' && projectNavIdx === getSelectedIndex()}
        <div>
          <button
            class="w-full px-2 mb-1 text-[11px] font-semibold text-t2 uppercase tracking-[.05em] truncate flex items-center gap-1.5 rounded-lg py-1 hover:bg-panel-hi {isProjectSelected ? '[box-shadow:inset_0_0_0_2px_color-mix(in_srgb,var(--theme-accent)_50%,transparent)]' : ''}"
            title={project.path}
            onclick={() => toggleSection(projectKey)}
            oncontextmenu={(e) => onProjectContextMenu(e, project)}
          >
            {#if projectCollapsed}<ChevronRight class="size-3 shrink-0 text-t3" />{:else}<ChevronDown class="size-3 shrink-0 text-t3" />{/if}
            {project.name}
            <span class="ml-auto font-normal text-t3">{(projectOrphans.length) + (projectTasks.length)}</span>
            {#if projectAutoMode[project.id]}<Zap class="size-2.5 text-status-running" />{/if}
          </button>

          {#if !projectCollapsed}

          <!-- Orphan sessions at top -->
          {#if projectOrphans.length > 0}
            <ul class="space-y-0.5 mb-1">
              {#each projectOrphans as session (session.id)}
                {@const globalIndex = flatNavIndex.get(`orphan:${session.id}`) ?? -1}
                {@const isActive = session.id === activeSessionId}
                {@const isSelected = zone === 'sidebar' && globalIndex === getSelectedIndex()}
                {@const isPreviewing = session.id === previewSessionId}
                <li>
                  {#if renamingSessionId === session.id}
                    <input
                      use:autofocus
                      class="w-full px-2 py-1.5 rounded-md text-sm bg-panel border border-accent outline-none text-t1"
                      bind:value={renameValue}
                      onkeydown={(e) => { if (e.key === 'Enter') commitRename(session.id); if (e.key === 'Escape') onStartRename(""); }}
                      onblur={() => commitRename(session.id)}
                    />
                  {:else}
                    <div class="flex items-center gap-1.5">
                      <span class="w-[2px] self-stretch rounded-full transition-opacity {isActive ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
                      <button
                        class="flex-1 min-w-0 text-left py-[6px] text-[13px] flex items-center gap-1.5 transition-colors rounded-lg px-2
                          {isActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                          {isPreviewing ? '[box-shadow:inset_0_0_0_2px_color-mix(in_srgb,var(--theme-accent)_50%,transparent)]' : isSelected ? '[box-shadow:inset_0_0_0_1px_color-mix(in_srgb,var(--theme-accent)_50%,transparent)]' : ''}"
                        onclick={() => handleOrphanClick(session)}
                        oncontextmenu={(e) => onContextMenu(e, session)}
                      >
                      <span class="truncate font-medium text-t1">{session.name || session.branch}</span>
                      <span class="ml-auto shrink-0 flex items-center gap-1.5">
                        {#if orchestrator.getReviewReady()[session.id]}
                          <Lightbulb class="size-3.5 text-status-review animate-pulse" />
                        {:else if session.status === 'exited'}
                          <span class="font-mono text-[9px] text-t3 bg-panel-hi rounded px-[5px] py-[1px]">exited</span>
                        {:else if agentStates[session.id] === 'Busy'}
                          <LoaderCircle class="size-3 animate-spin text-t2" />
                        {/if}
                        {@render ciBadge(session.id)}
                      </span>
                    </button>
                    </div>
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
              {@const statusNavIdx = flatNavIndex.get(`status:${project.path}:${status}`) ?? -1}
              {@const isStatusSelected = zone === 'sidebar' && statusNavIdx === getSelectedIndex()}
              <div>
                <button
                  class="w-full flex items-center gap-1.5 px-2 py-1 text-[9.5px] font-semibold text-t2 uppercase tracking-[.05em] hover:opacity-80 rounded-lg {isStatusSelected ? '[box-shadow:inset_0_0_0_1px_color-mix(in_srgb,var(--theme-accent)_50%,transparent)]' : ''}"
                  onclick={() => toggleSection(sectionKey)}
                >
                  <span class="size-1.5 rounded-full {statusDotColors[status]}"></span>
                  {statusLabels[status]}
                  <span class="font-normal text-t3 ml-0.5">{items.length}</span>
                  {#if collapsedSections[sectionKey]}<ChevronRight class="size-3 ml-auto text-t3" />{:else}<ChevronDown class="size-3 ml-auto text-t3" />{/if}
                </button>
                {#if !collapsedSections[sectionKey]}
                  <ul class="space-y-0.5">
                    {#each items as task (task.key)}
                      {@const linked = sessionForTask(task.key)}
                      {@const isActive = linked?.id === activeSessionId}
                      {@const taskNavIdx = flatNavIndex.get(`task:${task.key}`) ?? -1}
                      {@const isSelected = zone === 'sidebar' && taskNavIdx === getSelectedIndex()}
                      {@const isParent = isParentTask(task, projectTasks)}
                      {@const isPreviewing = linked && linked.id === previewSessionId}
                      <li>
                        <div class="flex items-center gap-1.5">
                          <span class="w-[2px] self-stretch rounded-full transition-opacity {isActive ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
                          <button
                            class="flex-1 min-w-0 text-left py-[6px] px-2 flex items-center gap-1.5 transition-colors rounded-lg
                              {isActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                              {isPreviewing ? '[box-shadow:inset_0_0_0_2px_color-mix(in_srgb,var(--theme-accent)_50%,transparent)]' : isSelected ? '[box-shadow:inset_0_0_0_1px_color-mix(in_srgb,var(--theme-accent)_50%,transparent)]' : ''}"
                            onclick={() => handleTaskClick(task, project.path)}
                            oncontextmenu={(e) => onTaskContextMenu(e, task, project.path)}
                          >
                          {#if task.parent_key}<span class="shrink-0 font-mono text-[10px] text-t3">{task.parent_key} ›</span>{/if}
                          <span class="shrink-0 font-mono text-[10px] text-t3">{task.key}</span>
                          <span class="truncate text-[12.5px] {task.status === 'done' ? 'line-through text-t3' : 'text-t1'}">{task.title}</span>
                          {#if linked}
                            <span class="ml-auto shrink-0 flex items-center gap-1.5">
                              {#if agentStates[linked.id] === 'Busy'}
                                <LoaderCircle class="size-3 animate-spin text-t2" />
                              {:else if orchestrator.getReviewReady()[linked.id]}
                                <Lightbulb class="size-3.5 text-status-review animate-pulse" />
                              {:else if linked.status === 'exited'}
                                <span class="font-mono text-[9px] text-t3 bg-panel-hi rounded px-[5px] py-[1px]">exited</span>
                              {/if}
                              {@render ciBadge(linked.id)}
                            </span>
                          {/if}
                        </button>
                        </div>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
          {/each}

          {#if projectOrphans.length === 0 && projectTasks.length === 0}
            <p class="px-3 py-1 text-xs text-t3 italic">No sessions or tasks</p>
          {/if}
          {/if}
        </div>
      {/each}
    {/if}
  </nav>

  <!-- Preferences footer -->
  <button
    onclick={onOpenPreferences}
    class="flex items-center gap-2 px-3 py-2.5 border-t border-border text-t2 text-[12px] hover:bg-panel-hi transition-colors"
  >
    <Settings class="size-3.5" />
    <span>Preferences</span>
    <span class="ml-auto font-mono text-[10px] text-t3">{MOD_LABEL},</span>
  </button>
</aside>

<!-- Hidden TaskPanel for create dialog -->
<div class="hidden">
  <TaskPanel
    bind:this={taskPanelRef}
    disableKeyboard={true}
    {onPickTask}
    {onSelectSession}
    onArchiveSession={async (s) => { const full = sessions.find(x => x.id === s.id); if (full) await onArchiveSession(full); }}
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
          { label: "Review", onSelect: () => { onSelectSession(contextMenu!.session.id); orchestrator.toggleDiff(); } },
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
      { label: "Archive project", onSelect: () => projectStore.archiveProject(projectContextMenu!.project.id) },
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
