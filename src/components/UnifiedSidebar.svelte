<script lang="ts">
  import { projects as projectsApi, jira as jiraApi } from "../lib/api";
  import { listen } from "@tauri-apps/api/event";
  import type { TaskItem, Session, Project } from "../lib/types";
  import { focusTerminal, getActiveZone, getSidebarSubZone } from "../lib/focus.svelte";
  import { getSelectedIndex, setSelectedIndex, clampIndex, handleSidebarKey } from "../lib/sidebar-nav.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { shouldHideProject, isLoopId, parseLoopId } from "../lib/sidebar-session-order";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ChevronDown, ChevronRight, LoaderCircle, Zap, Plus, CheckCircle2, XCircle, Lightbulb, Settings, MessageSquare, Play, Square } from "@lucide/svelte";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { MOD_LABEL } from "../lib/keyboard";
  import { getPreviewId } from "../lib/session-nav-cycle.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import JiraSidebarSection from "./JiraSidebarSection.svelte";
  import AssignJiraDialog from "./AssignJiraDialog.svelte";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import { getCiStatus } from "../lib/ci-checks.svelte";
  import { getCommentCount } from "../lib/pr-comments.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";
  import * as jiraTaskStore from "../lib/jira-task-store.svelte";
  import * as loopStore from "../lib/loop-store.svelte";

  interface Props {
    renamingSessionId: string | null;
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
    onSessionsChanged?: () => void;
    onAssignJiraTask?: (jiraTaskKey: string) => void;
    onSelectLoop?: (loopId: string) => void;
    onStartLoop?: (loopId: string) => void;
    onTickLoop?: (loopId: string) => void;
    onStopLoop?: (loopId: string) => void;
    onDeleteLoop?: (loopId: string) => void;
    onDeleteLoopSession?: (session: Session, loopId: string) => void;
    selectedLoopId?: string | null;
  }

  let { renamingSessionId, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onRestartSession, onOpenPreferences, onRenameSession, onStartRename, onDeleteProject, onPickTask, onCreateSession, onSessionsChanged, onAssignJiraTask, onSelectLoop, onStartLoop, onTickLoop, onStopLoop, onDeleteLoop, onDeleteLoopSession, selectedLoopId = null }: Props = $props();

  // ─── Derived from stores ────────────────────────────────────────────────────
  const projects = $derived(projectStore.getProjects());
  const sessions = $derived(orchestrator.getSessions());
  const activeSessionId = $derived(orchestrator.getActiveSessionId());
  const agentStates = $derived(orchestrator.getAgentStates());
  const zone = $derived(getActiveZone());
  const tasksByProject = $derived(taskStore.getTasksByProject());
  const jiraTasks = $derived(jiraTaskStore.getJiraTasks());
  const jiraChildCounts = $derived(jiraTaskStore.getChildCounts());

  // Aggregate all loops across all projects
  const allLoops = $derived(projects.flatMap((p) => loopStore.getLoopsForProject(p.id)));

  // Loop session data for nesting under parent loops
  const loopSessionsMap = $derived.by(() => {
    const map: Record<string, import("../lib/types").LoopSessionItem[]> = {};
    for (const loop of allLoops) {
      const items = loopStore.getSessionsForLoop(loop.id);
      if (items.length > 0) map[loop.id] = items;
    }
    return map;
  });

  // Set of session IDs that belong to a loop (used to exclude from orphan list)
  const loopSessionIds = $derived(new Set(
    Object.values(loopSessionsMap).flatMap((items) => items.map((i) => i.session_id))
  ));

  let navRef = $state<HTMLElement | undefined>(undefined);
  let sidebarWidth = $state(getLayoutWidth("sidebar", 266));
  let collapsedSections = $state<Record<string, boolean>>({ done: true });
  let renameValue = $state("");
  let searchQuery = $state("");
  let fadingSessionIds = $state<Set<string>>(new Set());

  const statusOrder = ["running", "needs_review", "idle", "todo", "done", "exited"];
  const statusLabels: Record<string, string> = { running: "Running", needs_review: "Needs review", idle: "Idle", todo: "To do", done: "Done", exited: "Exited" };
  const statusDotColors: Record<string, string> = { running: "bg-status-running", needs_review: "bg-status-review", idle: "bg-status-idle", todo: "bg-t3", done: "bg-status-running", exited: "bg-status-exited" };

  // Task status options for context menu (maps internal values to display labels)
  const STATUS_OPTIONS = [
    { value: "todo", label: "Todo" },
    { value: "in_progress", label: "In Progress" },
    { value: "in_review", label: "In Review" },
    { value: "done", label: "Done" },
  ] as const;

  // Auto-mode per project
  let projectAutoMode = $state<Record<string, boolean>>({});
  async function loadAutoModes() {
    for (const p of projects) {
      try { projectAutoMode[p.id] = await projectsApi.getAutoMode(p.id); } catch { /* ignore */ }
    }
  }
  $effect(() => { if (projects.length) loadAutoModes(); });
  let jiraConnected = $state(false);
  $effect(() => { jiraApi.status().then(s => { jiraConnected = s.connected; if (s.connected) jiraTaskStore.loadJiraTasks(); }); });
  $effect(() => {
    const unlisten = listen("jira-sync-complete", () => { jiraTaskStore.loadJiraTasks(); });
    return () => { unlisten.then(fn => fn()); };
  });
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
  let loopContextMenu = $state<{ x: number; y: number; loop: import("../lib/types").LoopRunSummary } | null>(null);
  let loopSessionContextMenu = $state<{ x: number; y: number; session: Session; loopId: string } | null>(null);

  // Loop helpers
  const loopStatusColors: Record<string, string> = {
    draft: "bg-t3", running: "bg-status-running", observing: "bg-status-running",
    verifying: "bg-status-running", completed_unreviewed: "bg-status-review",
    blocked: "bg-status-exited", needs_human: "bg-status-review", stale: "bg-status-exited",
    failed: "bg-status-exited", cancelled: "bg-status-exited", approved: "bg-status-running",
    merged: "bg-status-idle", cleaned: "bg-status-idle",
  };
  function loopStatusColor(status: string): string { return loopStatusColors[status] ?? "bg-t3"; }
  function isLoopActive(status: string): boolean { return ["running", "observing", "verifying"].includes(status); }
  function shortId(id: string): string { return id.slice(0, 8); }

  // Jira task assignment
  let assignTask = $state<TaskItem | null>(null);
  let assignPreselectedProjectId = $state("");
  let pendingAssignTask = $state<TaskItem | null>(null);
  let projectIdsBeforeCreate = $state<Set<string>>(new Set());

  function openAssignDialog(task: TaskItem) { assignTask = task; assignPreselectedProjectId = ""; }

  function startNewProjectForAssign() {
    if (assignTask) {
      pendingAssignTask = assignTask;
      projectIdsBeforeCreate = new Set(projects.map(p => p.id));
    }
    assignTask = null;
    onAddProject();
  }

  // Re-open assign dialog after project creation with the new project pre-selected
  $effect(() => {
    if (!pendingAssignTask) return;
    const newProject = projects.find(p => !projectIdsBeforeCreate.has(p.id));
    if (newProject) {
      assignTask = pendingAssignTask;
      assignPreselectedProjectId = newProject.id;
      pendingAssignTask = null;
    }
  });

  function onContextMenu(e: MouseEvent, session: Session) { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, session }; }
  function onProjectContextMenu(e: MouseEvent, project: Project) { e.preventDefault(); projectContextMenu = { x: e.clientX, y: e.clientY, project }; }
  function onTaskContextMenu(e: MouseEvent, task: TaskItem, projectPath: string) { e.preventDefault(); taskContextMenu = { x: e.clientX, y: e.clientY, task, projectPath }; }

  // External triggers
  let taskPanelRef = $state<TaskPanel | undefined>(undefined);

  // Derive orphan sessions (no task_key or task_key not in loaded tasks, and not in a loop)
  const allTaskKeys = $derived(new Set(Object.values(tasksByProject).flat().map(t => t.key)));
  const orphanSessions = $derived(sessions.filter(s => (!s.task_key || !allTaskKeys.has(s.task_key)) && !loopSessionIds.has(s.id)));
  const orphansByProject = $derived(
    projects.map(p => ({ project: p, sessions: orphanSessions.filter(s => s.project_id === p.id) })).filter(g => g.sessions.length > 0)
  );

  const previewId = $derived(getPreviewId());
  /** If previewing a loop dashboard, this is the loop ID; otherwise null */
  const previewLoopId = $derived(previewId && isLoopId(previewId) ? parseLoopId(previewId) : null);
  /** If previewing a session (not a loop), this is the session ID; otherwise null */
  const previewSessionId = $derived(previewId && !isLoopId(previewId) ? previewId : null);

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

  function fadeOutThenAct(sessionId: string, action: () => void | Promise<void>) {
    fadingSessionIds = new Set([...fadingSessionIds, sessionId]);
    setTimeout(() => {
      fadingSessionIds = new Set([...fadingSessionIds].filter(id => id !== sessionId));
      Promise.resolve(action()).catch((err) => {
        console.error("fadeOutThenAct action failed:", err);
      });
    }, 200);
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

  // Filter projects based on hide_empty_projects setting
  const visibleProjects = $derived(
    projects.filter(p => {
      const orphans = orphansByProject.find(g => g.project.id === p.id)?.sessions ?? [];
      const tasks = tasksByProject[p.path] ?? [];
      const visibleTaskCount = tasks.filter(t => !(t.status === "done" && getSettings().hide_done_tasks)).length;
      const loopCount = loopStore.getLoopsForProject(p.id).length;
      return !shouldHideProject(orphans.length, visibleTaskCount, !!getSettings().hide_empty_projects, loopCount);
    })
  );

  // Flat nav list for keyboard navigation
  type NavItem = { type: "project_header"; project: Project } | { type: "loop"; loop: import("../lib/types").LoopRunSummary } | { type: "loop_session"; session: Session; loopId: string; item: import("../lib/types").LoopSessionItem } | { type: "orphan"; session: Session } | { type: "status_header"; projectPath: string; status: string } | { type: "task"; task: TaskItem; projectPath: string } | { type: "jira_header" } | { type: "jira_task"; task: TaskItem };
  const flatNav = $derived.by(() => {
    const result: NavItem[] = [];
    for (const project of visibleProjects) {
      const projectKey = `project:${project.id}`;
      result.push({ type: "project_header", project });
      if (isProjectCollapsed(project)) continue;
      // Loops first (with their child sessions)
      const projectLoops = loopStore.getLoopsForProject(project.id);
      for (const loop of projectLoops) {
        result.push({ type: "loop", loop });
        const loopKey = `loop:${loop.id}`;
        if (collapsedSections[loopKey]) continue;
        const children = loopSessionsMap[loop.id] ?? [];
        for (const item of children) {
          const session = sessions.find(s => s.id === item.session_id);
          if (session) result.push({ type: "loop_session", session, loopId: loop.id, item });
        }
      }
      // Orphans
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
    // Jira section
    if (jiraTasks.length > 0) {
      result.push({ type: "jira_header" });
      if (!collapsedSections["jira"]) {
        for (const t of jiraTasks) result.push({ type: "jira_task", task: t });
      }
    }
    return result;
  });

  // O(1) lookup map for nav index (avoids O(n²) findIndex in template)
  const flatNavIndex = $derived.by(() => {
    const map = new Map<string, number>();
    flatNav.forEach((item, i) => {
      if (item.type === "project_header") map.set(`project:${item.project.id}`, i);
      else if (item.type === "loop") map.set(`loop:${item.loop.id}`, i);
      else if (item.type === "loop_session") map.set(`loop_session:${item.session.id}`, i);
      else if (item.type === "orphan") map.set(`orphan:${item.session.id}`, i);
      else if (item.type === "status_header") map.set(`status:${item.projectPath}:${item.status}`, i);
      else if (item.type === "task") map.set(`task:${item.task.key}`, i);
      else if (item.type === "jira_header") map.set("jira_header", i);
      else if (item.type === "jira_task") map.set(`jira:${item.task.key}`, i);
    });
    return map;
  });

  $effect(() => { clampIndex(flatNav.length); });

  // Scroll selected item into view on keyboard navigation
  $effect(() => {
    const idx = getSelectedIndex();
    if (zone !== "sidebar" || !navRef) return;
    navRef.querySelector(`[data-nav-index="${idx}"]`)?.scrollIntoView({ block: "nearest" });
  });

  // Auto-focus active session/loop when sessions panel is toggled
  $effect(() => {
    if (zone !== "sidebar" || getSidebarSubZone() !== "sessions") return;
    // If viewing a loop dashboard, highlight the loop item
    if (selectedLoopId) {
      const idx = flatNavIndex.get(`loop:${selectedLoopId}`);
      if (idx !== undefined) setSelectedIndex(idx);
      return;
    }
    if (!activeSessionId) return;
    // Try loop_session lookup first
    let idx = flatNavIndex.get(`loop_session:${activeSessionId}`);
    // Then orphan lookup
    if (idx === undefined) idx = flatNavIndex.get(`orphan:${activeSessionId}`);
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
      } else if (current.type === "loop") {
        const loopKey = `loop:${current.loop.id}`;
        if (!collapsedSections[loopKey]) collapsedSections = { ...collapsedSections, [loopKey]: true };
      } else if (current.type === "loop_session") {
        // Jump to parent loop
        const loopIdx = flatNavIndex.get(`loop:${current.loopId}`);
        if (loopIdx !== undefined) setSelectedIndex(loopIdx);
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
      } else if (current.type === "jira_header") {
        if (!collapsedSections["jira"]) collapsedSections = { ...collapsedSections, jira: true };
      } else if (current.type === "jira_task") {
        // First press jumps to header; if already on header it will collapse
        const jiraIdx = flatNav.findIndex(n => n.type === "jira_header");
        if (jiraIdx >= 0) setSelectedIndex(jiraIdx);
      }
      return;
    }

    if (action.type === "expand") {
      if (current.type === "project_header") {
        const key = `project:${current.project.id}`;
        if (collapsedSections[key]) collapsedSections = { ...collapsedSections, [key]: false };
      } else if (current.type === "loop") {
        const loopKey = `loop:${current.loop.id}`;
        if (collapsedSections[loopKey]) collapsedSections = { ...collapsedSections, [loopKey]: false };
      } else if (current.type === "status_header") {
        const sectionKey = `${current.projectPath}:${current.status}`;
        if (collapsedSections[sectionKey]) collapsedSections = { ...collapsedSections, [sectionKey]: false };
      } else if (current.type === "jira_header") {
        if (collapsedSections["jira"]) collapsedSections = { ...collapsedSections, jira: false };
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

    if (current.type === "jira_header") {
      if (action.type === "select") toggleSection("jira");
      return;
    }

    if (current.type === "jira_task") {
      if (action.type === "select") openAssignDialog(current.task);
      return;
    }

    if (current.type === "loop") {
      if (action.type === "select") { onSelectLoop?.(current.loop.id); }
      else if (action.type === "delete") { onDeleteLoop?.(current.loop.id); }
      return;
    }

    if (current.type === "loop_session") {
      if (action.type === "select") { onSelectSession(current.session.id); focusTerminal(); }
      else if (action.type === "delete") { onDeleteLoopSession?.(current.session, current.loopId); }
      return;
    }

    if (current.type === "orphan") {
      const session = current.session;
      if (action.type === "select") { onSelectSession(session.id); focusTerminal(); }
      else if (action.type === "archive") fadeOutThenAct(session.id, () => onArchiveSession(session));
      else if (action.type === "delete") fadeOutThenAct(session.id, () => onDeleteSession(session));
      else if (action.type === "rename") startRename(session);
      else if (action.type === "restart") onRestartSession(session);
      else if (action.type === "open_pr") { if (session.pr_url) openUrl(session.pr_url); }
      else if (action.type === "review") { onSelectSession(session.id); orchestrator.toggleDiff(); }
    } else if (current.type === "task") {
      const task = current.task;
      if (action.type === "select" || action.type === "start_session") handleTaskClick(task, current.projectPath);
      else if (action.type === "edit") taskPanelRef?.openEdit(task);
      else if (action.type === "status") moveTask(task.key, action.status);
      else if (action.type === "open_pr") { const linked = sessionForTask(task.key); if (linked?.pr_url) openUrl(linked.pr_url); }
      else if (action.type === "review") { const linked = sessionForTask(task.key); if (linked) { onSelectSession(linked.id); orchestrator.toggleDiff(); } }
      else if (action.type === "archive") { const linked = sessionForTask(task.key); if (linked) fadeOutThenAct(linked.id, () => onArchiveSession(linked)); }
      else if (action.type === "delete") { const linked = sessionForTask(task.key); if (linked) onDeleteSession(linked); }
      else if (action.type === "rename") { const linked = sessionForTask(task.key); if (linked) startRename(linked); }
      else if (action.type === "restart") { const linked = sessionForTask(task.key); if (linked) onRestartSession(linked); }
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
    if (jiraConnected) jiraTaskStore.loadJiraTasks();
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
    <span class="size-2 rounded-full bg-amber-500" style="animation:pulse-dot 1.6s ease-in-out infinite" title="CI running"></span>
  {/if}
{/snippet}

{#snippet commentBadge(id: string)}
  {@const count = getCommentCount(id)}
  {#if count > 0}
    <span class="flex items-center gap-0.5 text-[10px] text-t3" title="{count} comment{count !== 1 ? 's' : ''}">
      <MessageSquare class="size-3" />{count}
    </span>
  {/if}
{/snippet}

<aside class="relative shrink-0 flex flex-col border-r bg-sidebar {zone === 'sidebar' ? 'border-accent' : 'border-border'}" style:width="{sidebarWidth}px">
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
  <nav bind:this={navRef} class="flex-1 overflow-y-auto overflow-x-hidden px-2 py-2 space-y-3 scrollbar-hide select-none">
    {#if projects.length === 0}
      <div class="mt-12 text-center px-4 space-y-3">
        <p class="text-xs text-t3">No projects yet</p>
        <button onclick={onAddProject} class="text-xs text-accent hover:underline">Add a project →</button>
      </div>
    {:else}
      {#each visibleProjects as project (project.id)}
        {@const projectTasks = tasksByProject[project.path] ?? []}
        {@const statusGroups = groupByStatus(projectTasks)}
        {@const projectOrphans = orphansByProject.find(g => g.project.id === project.id)?.sessions ?? []}
        {@const projectKey = `project:${project.id}`}
        {@const projectCollapsed = isProjectCollapsed(project)}
        {@const projectNavIdx = flatNavIndex.get(`project:${project.id}`) ?? -1}
        {@const isProjectSelected = zone === 'sidebar' && projectNavIdx === getSelectedIndex()}
        <div>
          <button
            data-nav-index={projectNavIdx}
            class="w-full px-2 mb-1 text-[11px] font-semibold text-t2 uppercase tracking-[.05em] truncate flex items-center gap-1.5 rounded-lg py-1 hover:bg-panel-hi {isProjectSelected ? 'ring-2 ring-accent' : ''}"
            title={project.path}
            onclick={() => toggleSection(projectKey)}
            oncontextmenu={(e) => onProjectContextMenu(e, project)}
          >
            {project.name}
            <span class="ml-auto font-normal text-t3">{(projectOrphans.length) + (projectTasks.length)}</span>
            {#if projectAutoMode[project.id]}<Zap class="size-2.5 text-status-running" />{/if}
            {#if projectCollapsed}<ChevronRight class="size-3 shrink-0 text-t3" />{:else}<ChevronDown class="size-3 shrink-0 text-t3" />{/if}
          </button>

          {#if !projectCollapsed}

          <!-- Loops at top of project -->
          {@const projectLoops = loopStore.getLoopsForProject(project.id)}
          {#if projectLoops.length > 0}
            <ul class="space-y-0.5 mb-1">
              {#each projectLoops as loop (loop.id)}
                {@const loopNavIdx = flatNavIndex.get(`loop:${loop.id}`) ?? -1}
                {@const isLoopSelected = zone === 'sidebar' && loopNavIdx === getSelectedIndex()}
                {@const isLoopDashboardActive = loop.id === selectedLoopId}
                {@const isLoopPreviewing = loop.id === previewLoopId}
                {@const loopKey = `loop:${loop.id}`}
                {@const loopCollapsed = collapsedSections[loopKey] ?? false}
                {@const childSessions = loopSessionsMap[loop.id] ?? []}
                <li>
                  <!-- Loop item (aligned with status section headers) -->
                  <div
                    role="button"
                    tabindex="0"
                    data-nav-index={loopNavIdx}
                    class="group w-full flex items-center gap-1.5 pl-2 pr-2 py-1 text-left transition-colors rounded-lg cursor-pointer
                      {isLoopDashboardActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                      {isLoopPreviewing ? 'ring-2 ring-accent' : isLoopSelected ? 'ring-2 ring-accent' : ''}"
                    onclick={() => onSelectLoop?.(loop.id)}
                    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectLoop?.(loop.id); } }}
                    oncontextmenu={(e) => { e.preventDefault(); loopContextMenu = { x: e.clientX, y: e.clientY, loop }; }}
                    title={loop.goal}
                  >
                    <!-- Status dot -->
                    <span class="size-1.5 rounded-full shrink-0 {loopStatusColor(loop.status)}"></span>
                    <!-- Label -->
                    {#if loop.task_key}
                      <span class="font-medium text-[12.5px] text-t1 truncate">{loop.task_key}</span>
                    {:else}
                      <span class="text-t3 font-mono text-xs truncate">{shortId(loop.id)}</span>
                    {/if}
                    <span class="text-t3 text-xs">{loop.strategy}</span>
                    <!-- Round limit -->
                    <span class="text-t3 text-xs shrink-0">max {loop.max_rounds} rounds</span>
                    <!-- Quick actions (show on hover) -->
                    <span class="hidden group-hover:flex items-center gap-0.5">
                      {#if loop.status === "draft"}
                        <button class="p-0.5 rounded hover:bg-panel-hi text-t3 hover:text-t1" onclick={(e) => { e.stopPropagation(); onStartLoop?.(loop.id); }} title="Start" aria-label="Start loop"><Play class="size-3" /></button>
                      {:else if isLoopActive(loop.status)}
                        <button class="p-0.5 rounded hover:bg-panel-hi text-t3 hover:text-t1" onclick={(e) => { e.stopPropagation(); onTickLoop?.(loop.id); }} title="Tick" aria-label="Tick loop"><Play class="size-3" /></button>
                        <button class="p-0.5 rounded hover:bg-panel-hi text-t3 hover:text-status-exited" onclick={(e) => { e.stopPropagation(); onStopLoop?.(loop.id); }} title="Stop" aria-label="Stop loop"><Square class="size-3" /></button>
                      {/if}
                    </span>
                    <!-- Chevron on right -->
                    {#if childSessions.length > 0}
                      {#if loopCollapsed}<ChevronRight class="size-3 ml-auto text-t3 shrink-0" />{:else}<ChevronDown class="size-3 ml-auto text-t3 shrink-0" />{/if}
                    {/if}
                  </div>
                  <!-- Indented child sessions -->
                  {#if !loopCollapsed && childSessions.length > 0}
                    <ul class="space-y-0.5 mt-0.5">
                      {#each childSessions as item (item.session_id)}
                        {@const childSession = sessions.find(s => s.id === item.session_id)}
                        {#if childSession}
                          {@const childNavIdx = flatNavIndex.get(`loop_session:${childSession.id}`) ?? -1}
                          {@const isChildActive = childSession.id === activeSessionId}
                          {@const isChildSelected = zone === 'sidebar' && childNavIdx === getSelectedIndex()}
                          {@const isChildPreviewing = childSession.id === previewSessionId}
                          <li>
                            <div class="flex items-center gap-1.5">
                              <span class="w-[2px] self-stretch rounded-full transition-opacity {isChildActive ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
                              <button
                                data-nav-index={childNavIdx}
                                class="flex-1 min-w-0 text-left py-[6px] text-[13px] flex items-center gap-1.5 transition-colors rounded-lg pl-4 pr-2
                                  {isChildActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                                  {isChildPreviewing ? 'ring-2 ring-accent' : isChildSelected ? 'ring-2 ring-accent' : ''}"
                                onclick={() => { onSelectSession(childSession.id); focusTerminal(); }}
                                oncontextmenu={(e) => { e.preventDefault(); loopSessionContextMenu = { x: e.clientX, y: e.clientY, session: childSession, loopId: loop.id }; }}
                              >
                                <span class="shrink-0 font-mono text-[10px] text-t3">{item.role}</span>
                                <span class="truncate font-medium text-t1">{childSession.name || childSession.branch}</span>
                                <span class="ml-auto shrink-0 flex items-center gap-1.5">
                                  {#if orchestrator.getReviewReady()[childSession.id]}
                                    <Lightbulb class="size-3.5 text-status-review animate-pulse" />
                                  {:else if childSession.status === 'exited'}
                                    <span class="font-mono text-[9px] text-t3 bg-panel-hi rounded px-[5px] py-[1px]">exited</span>
                                  {:else if agentStates[childSession.id] === 'Busy'}
                                    <LoaderCircle class="size-3 animate-spin text-t2" />
                                  {/if}
                                </span>
                              </button>
                            </div>
                          </li>
                        {/if}
                      {/each}
                    </ul>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}

          <!-- Orphan sessions at top -->
          {#if projectOrphans.length > 0}
            <ul class="space-y-0.5 mb-1">
              {#each projectOrphans as session (session.id)}
                {@const globalIndex = flatNavIndex.get(`orphan:${session.id}`) ?? -1}
                {@const isActive = session.id === activeSessionId}
                {@const isSelected = zone === 'sidebar' && globalIndex === getSelectedIndex()}
                {@const isPreviewing = session.id === previewSessionId}
                <li class="transition-opacity duration-200 {fadingSessionIds.has(session.id) ? 'opacity-0' : 'opacity-100'}">
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
                        data-nav-index={globalIndex}
                        class="flex-1 min-w-0 text-left py-[6px] text-[13px] flex items-center gap-1.5 transition-colors rounded-lg pl-4 pr-2
                          {isActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                          {isPreviewing ? 'ring-2 ring-accent' : isSelected ? 'ring-2 ring-accent' : ''}"
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
                        {@render commentBadge(session.id)}
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
                  data-nav-index={statusNavIdx}
                  class="w-full flex items-center gap-1.5 pl-2 pr-2 py-1 text-[9.5px] font-semibold text-t2 uppercase tracking-[.05em] hover:opacity-80 rounded-lg {isStatusSelected ? 'ring-2 ring-accent' : ''}"
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
                      <li class="transition-opacity duration-200 {linked && fadingSessionIds.has(linked.id) ? 'opacity-0' : 'opacity-100'}">
                        <div class="flex items-center gap-1.5">
                          <span class="w-[2px] self-stretch rounded-full transition-opacity {isActive ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
                          <button
                            data-nav-index={taskNavIdx}
                            class="flex-1 min-w-0 text-left py-[6px] pl-4 pr-2 flex items-center gap-1.5 transition-colors rounded-lg
                              {isActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                              {isPreviewing ? 'ring-2 ring-accent' : isSelected ? 'ring-2 ring-accent' : ''}"
                            onclick={() => handleTaskClick(task, project.path)}
                            oncontextmenu={(e) => onTaskContextMenu(e, task, project.path)}
                          >
                          {#if task.parent_key}<span class="shrink-0 font-mono text-[10px] text-t3">{task.parent_key} ›</span>{/if}
                          <span class="shrink-0 font-mono text-[10px] {linked ? 'text-accent/70' : 'text-t3'}">{task.key}</span>
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
                              {@render commentBadge(linked.id)}
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

      <!-- Jira section -->
      <JiraSidebarSection
        tasks={jiraTasks}
        childCounts={jiraChildCounts}
        collapsed={collapsedSections["jira"] ?? false}
        {zone}
        {flatNavIndex}
        onToggleSection={() => toggleSection("jira")}
        onAssignJiraTask={(key) => {
          const task = jiraTasks.find(t => t.key === key);
          if (task) openAssignDialog(task);
        }}
      />
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

<!-- Hidden TaskPanel for edit dialog -->
<div class="absolute w-0 h-0 overflow-hidden">
  <TaskPanel
    bind:this={taskPanelRef}
    disableKeyboard={true}
    {onPickTask}
    {onSelectSession}
    onArchiveSession={async (s) => { const full = sessions.find(x => x.id === s.id); if (full) fadeOutThenAct(full.id, () => onArchiveSession(full)); }}
    onSessionsChanged={onSessionsChanged}
  />
</div>

<!-- Session context menu -->
{#if contextMenu}
  {@const menuSession = contextMenu.session}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={menuSession.status === 'exited'
      ? [
          { label: "Restart", onSelect: () => onRestartSession(menuSession) },
          { label: "Rename", onSelect: () => startRename(menuSession) },
          { label: "Archive", onSelect: () => fadeOutThenAct(menuSession.id, () => onArchiveSession(menuSession)) },
          { label: "Delete", danger: true, onSelect: () => fadeOutThenAct(menuSession.id, () => onDeleteSession(menuSession)) },
        ]
      : [
          { label: "Review", onSelect: () => { onSelectSession(menuSession.id); orchestrator.toggleDiff(); } },
          { label: "Rename", onSelect: () => startRename(menuSession) },
          { label: "Archive", onSelect: () => fadeOutThenAct(menuSession.id, () => onArchiveSession(menuSession)) },
          { label: "Delete", danger: true, onSelect: () => fadeOutThenAct(menuSession.id, () => onDeleteSession(menuSession)) },
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
  {@const menuTask = taskContextMenu.task}
  {@const linkedSession = sessionForTask(menuTask.key)}
  {@const statusChildren = STATUS_OPTIONS
    .filter(s => s.value !== menuTask.status)
    .map(s => ({ label: s.label, onSelect: () => moveTask(menuTask.key, s.value) }))}
  <ContextMenu
    x={taskContextMenu.x}
    y={taskContextMenu.y}
    onClose={() => (taskContextMenu = null)}
    items={[
      ...(linkedSession
        ? [{ label: "Review diff", onSelect: () => { onSelectSession(linkedSession.id); orchestrator.toggleDiff(); } }]
        : []),
      { label: "Edit task", onSelect: () => taskPanelRef?.openEdit(menuTask) },
      { label: "Change status", children: statusChildren },
      ...(linkedSession
        ? [
            ...(linkedSession.status === 'exited' ? [{ label: "Restart session", onSelect: () => onRestartSession(linkedSession) }] : []),
            { label: "Rename session", onSelect: () => startRename(linkedSession) },
            { label: "Archive session", onSelect: () => fadeOutThenAct(linkedSession.id, () => onArchiveSession(linkedSession)) },
            { label: "Delete session", danger: true, onSelect: () => onDeleteSession(linkedSession) },
          ]
        : []),
    ]}
  />
{/if}

<!-- Loop context menu -->
{#if loopContextMenu}
  <ContextMenu
    x={loopContextMenu.x}
    y={loopContextMenu.y}
    onClose={() => (loopContextMenu = null)}
    items={[
      ...(loopContextMenu.loop.status === "draft" ? [{ label: "Start loop", onSelect: () => onStartLoop?.(loopContextMenu!.loop.id) }] : []),
      ...(isLoopActive(loopContextMenu.loop.status) ? [{ label: "Stop loop", onSelect: () => onStopLoop?.(loopContextMenu!.loop.id) }] : []),
      { label: "Delete loop", danger: true, onSelect: () => onDeleteLoop?.(loopContextMenu!.loop.id) },
    ]}
  />
{/if}

<!-- Loop session context menu -->
{#if loopSessionContextMenu}
  <ContextMenu
    x={loopSessionContextMenu.x}
    y={loopSessionContextMenu.y}
    onClose={() => (loopSessionContextMenu = null)}
    items={[
      { label: "Review", onSelect: () => { onSelectSession(loopSessionContextMenu!.session.id); orchestrator.toggleDiff(); } },
      { label: "Delete", danger: true, onSelect: () => { onDeleteLoopSession?.(loopSessionContextMenu!.session, loopSessionContextMenu!.loopId); loopSessionContextMenu = null; } },
    ]}
  />
{/if}

<!-- Assign Jira task to project dialog -->
{#if assignTask}
<AssignJiraDialog
  task={assignTask}
  {projects}
  preselectedProjectId={assignPreselectedProjectId}
  onClose={() => { assignTask = null; }}
  onNewProject={startNewProjectForAssign}
/>
{/if}
