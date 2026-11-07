<script lang="ts">
  import { projects as projectsApi } from "../lib/api";
  import { sessionTaskProjectId, type TaskItem, type Session, type Project, type PluginSessionAction, type PluginSlot } from "../lib/types";
  import type { PluginSessionContext } from "../lib/plugin-sdk";
  import { pluginSessionActionsForProvider } from "../lib/plugin-session-actions";
  import { focusSidebar, focusTerminal, getActiveZone, getSidebarSubZone } from "../lib/focus.svelte";
  import { deepActiveElement, releaseFrameFocus } from "../lib/dom-focus";
  import { getSelectedIndex, setSelectedIndex, clampIndex, handleSidebarKey, shouldBypassSidebarKeyboard, activatesItselfOnEnter, focusedSidebarControl } from "../lib/sidebar-nav.svelte";
  import { getSettings, updateSettings } from "../lib/settings.svelte";
  import { isTaskWorkspaceId, parseTaskWorkspaceId } from "../lib/sidebar-session-order";
  import { everyLane, LANES, navKey, PROJECT_GROUP_LANES, projectLabelsApply, resolveSidebarGroupBy, resolveSidebarLayout, sidebarNavItemKey, sidebarNavItems, STATUS_SECTION_LANES, taskHeaderNavKeys, type SidebarLaneGroup, type SidebarNavItem, type SidebarProjectSection, type SidebarRow, type SidebarSection, type SidebarSessionRow, type SidebarTaskRow, type TaskLane } from "../lib/sidebar-model";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { crossfade } from "svelte/transition";
  import { laneHover, liftWhileDragged, pressTaskRow, taskDrag, taskDragPayload, type TaskDragPayload, type TaskDropTarget } from "../lib/task-drag.svelte";
  import { laneVerdict } from "../lib/task-drop";
  import LaneDropBadge from "./sidebar/LaneDropBadge.svelte";
  import { rowActivity, rowSession, WOKE_TITLE, type RowAction, type RowActivity } from "../lib/sidebar-cards";
  import { getSidebarModel, isCollapsed, isOpenRow, setCollapsed, shownPages, showMore } from "../lib/sidebar-model-store.svelte";
  import { sidebarViewMenuItems } from "../lib/sidebar-view-menu";
  import { projectContextMenuItems } from "../lib/project-context-menu";
  import { locateProjectFolder } from "../lib/locate-project";
  import { AlarmClock, ChevronDown, ChevronRight, LoaderCircle, Zap, Plus, FolderPlus, Lightbulb, Settings, TriangleAlert } from "@lucide/svelte";
  import PluginContributionHost from "./PluginContributionHost.svelte";
  import SessionRenameInput from "./sidebar/SessionRenameInput.svelte";
  import JumpHintBadge from "./sidebar/JumpHintBadge.svelte";
  import SidebarCardsList from "./sidebar/SidebarCardsList.svelte";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { isTextEntryElement, MOD_LABEL } from "../lib/keyboard";
  import { getPreviewId } from "../lib/session-nav-cycle.svelte";
  import { installJumpHints, jumpHintFor } from "../lib/jump-hints.svelte";
  import { showToast } from "../lib/toasts.svelte";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";
  import { changeTaskStatus, wakeAndChangeTaskStatus, wakeTask } from "../lib/task-moves";
  import { getLaneNow } from "../lib/snooze-clock.svelte";
  import SnoozeCountdown from "./sidebar/SnoozeCountdown.svelte";
  import { snoozeAnchor, snoozeMenuEntry, snoozeOrWake } from "../lib/snooze-picker.svelte";
  import { isTaskUnread } from "../lib/task-unread";
  import { getPluginSidebarRows, type PluginSidebarNavRow } from "../lib/plugin-sidebar-navigation.svelte";
  import { pluginPreferencesLocation, type SettingsLocation } from "../lib/settings-registry";

  interface Props {
    renamingSessionId: string | null;
    onAddProject: () => void;
    onSelectSession: (id: string) => void;
    onArchiveSession: (session: Session) => void;
    onDeleteSession: (session: Session) => void;
    onRestartSession: (session: Session) => void;
    onOpenPreferences: (location?: SettingsLocation) => void;
    onRenameSession: (id: string, name: string) => void;
    onStartRename: (id: string) => void;
    onDeleteProject: (project: Project) => void;
    onEditProject: (project: Project) => void;
    onEditTask: (task: TaskItem, project: Project) => void;
    onPickTask: (task: TaskItem, repoPath: string) => void;
    onSelectTask?: (task: TaskItem, repoPath: string) => void;
    /** A task row dropped on a session pane, to tell that agent about it. */
    onReferenceTask?: (task: TaskDragPayload, paneId: string) => void;
    onCreateTask?: () => void;
    onSessionsChanged?: () => void;
    onToggleDiff?: () => void;
    pluginContributions?: PluginSlot[];
    sessionIndicatorContributions?: PluginSlot[];
    /** Rendered on Cards rows only; Classic rows have no room for them. */
    sessionMetaContributions?: PluginSlot[];
    pluginSessionActions?: PluginSessionAction[];
    onPluginSessionAction?: (session: Session, action: PluginSessionAction) => void;
    onPluginNavigate?: (pluginId: string, contributionId: string) => void;
    onPluginClose?: () => void;
    /** Selects a session and opens a plugin's session panel for it, from the plugin's meta slot. */
    onOpenPluginSessionPanel?: (sessionId: string, pluginId: string, contributionId: string) => void;
  }

  let { renamingSessionId, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onRestartSession, onOpenPreferences, onRenameSession, onStartRename, onDeleteProject, onEditProject, onEditTask, onPickTask, onSelectTask = () => {}, onReferenceTask, onCreateTask, onSessionsChanged, onToggleDiff, pluginContributions = [], sessionIndicatorContributions = [], sessionMetaContributions = [], pluginSessionActions = [], onPluginSessionAction, onPluginNavigate, onPluginClose, onOpenPluginSessionPanel }: Props = $props();
  let failedSidebarContributions = $state<Set<string>>(new Set());

  function pluginContributionKey(item: PluginSlot): string {
    return `${item.plugin.id}:${item.contribution.id}`;
  }

  function markSidebarContributionFailed(item: PluginSlot): void {
    const key = pluginContributionKey(item);
    if (failedSidebarContributions.has(key)) return;
    failedSidebarContributions = new Set([...failedSidebarContributions, key]);
  }

  function pluginSessionContext(session: Session): PluginSessionContext {
    return {
      id: session.id,
      projectId: session.project_id,
      branch: session.branch,
      baseBranch: session.base_branch,
      status: session.status as PluginSessionContext["status"],
      provider: session.provider,
      taskKey: session.task_key,
    };
  }

  const activePluginContributions = $derived(
    pluginContributions.filter((item) => !failedSidebarContributions.has(pluginContributionKey(item))),
  );

  // ─── Derived from stores ────────────────────────────────────────────────────
  const projects = $derived(projectStore.getProjects());
  const sessions = $derived(orchestrator.getSessions());
  const activeSessionId = $derived(orchestrator.getActiveSessionId());
  const agentStates = $derived(orchestrator.getAgentStates());
  const unreadSessions = $derived(orchestrator.getUnreadSessions());
  const now = $derived(getLaneNow());
  const zone = $derived(getActiveZone());
  const sections = $derived(getSidebarModel());
  const groupBy = $derived(resolveSidebarGroupBy(getSettings()));
  const layout = $derived(resolveSidebarLayout(getSettings()));
  const showTaskKeys = $derived(!getSettings().hide_task_keys);
  const showProjectLabels = $derived(projectLabelsApply(getSettings()) && !getSettings().hide_project_labels);

  let navRef = $state<HTMLElement | undefined>(undefined);
  let sidebarWidth = $state(getLayoutWidth("sidebar", 266));
  let renameValue = $state("");
  let fadingSessionIds = $state<Set<string>>(new Set());

  // Below this width the labelled New project button truncates to "Ne…".
  const compactProjectButton = $derived(projects.length > 0 && sidebarWidth < 400);
  // Justify and padding are set per button: Tailwind does not resolve conflicting utilities by class order.
  const headerBtnClass = "h-[32px] flex items-center rounded-lg bg-panel-hi border border-border text-t2 text-[12px] font-medium hover:opacity-80 transition-opacity min-w-0";

  // Task status options for context menu (maps internal values to display labels)
  const STATUS_OPTIONS = [
    { value: "todo", label: "Todo" },
    { value: "in_progress", label: "In Progress" },
    { value: "in_review", label: "In Review" },
    { value: "done", label: "Done" },
  ] as const;

  $effect(installJumpHints);

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
  let taskContextMenu = $state<{ x: number; y: number; row: SidebarTaskRow } | null>(null);
  let viewMenu = $state<{ x: number; y: number } | null>(null);
  async function hideProject(id: string) {
    try {
      await projectStore.hideProject(id);
    } catch (error) {
      console.error("Failed to hide project:", error);
      showToast("Failed to hide project.");
    }
  }

  function onContextMenu(e: MouseEvent, session: Session) { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, session }; }
  function onProjectContextMenu(e: MouseEvent, project: Project) { e.preventDefault(); projectContextMenu = { x: e.clientX, y: e.clientY, project }; }
  function onTaskContextMenu(e: MouseEvent, row: SidebarTaskRow) { e.preventDefault(); taskContextMenu = { x: e.clientX, y: e.clientY, row }; }

  // Empty space and group headers open view options. Rows, inputs, project headers and plugin slots keep their own menus.
  function onNavContextMenu(e: MouseEvent) {
    if (e.defaultPrevented) return;
    if (e.composedPath().some((node) => node instanceof HTMLElement && (node.tagName === "LI" || isTextEntryElement(node) || node.dataset.pluginSidebarSlot !== undefined))) return;
    e.preventDefault();
    viewMenu = { x: e.clientX, y: e.clientY };
  }

  // External triggers

  const previewId = $derived(getPreviewId());
  const previewTaskWorkspace = $derived(
    previewId && isTaskWorkspaceId(previewId) ? parseTaskWorkspaceId(previewId) : null,
  );
  /** If previewing a session, this is the session ID; otherwise null */
  const previewSessionId = $derived(previewId && !isTaskWorkspaceId(previewId) ? previewId : null);

  function toggleSection(key: string, fallback: boolean) {
    setCollapsed(key, !isCollapsed(key, fallback), fallback);
  }

  function handleTaskClick(task: TaskItem, project: Project) {
    onSelectTask(task, project.path);
    focusTerminal();
  }

  function handleOrphanClick(session: Session) {
    onSelectSession(session.id);
    focusTerminal();
  }

  function selectRow(row: SidebarRow) {
    if (row.kind === "task") handleTaskClick(row.task, row.project);
    else handleOrphanClick(row.session);
  }

  function onRowContextMenu(e: MouseEvent, row: SidebarRow) {
    if (row.kind === "task") onTaskContextMenu(e, row);
    else onContextMenu(e, row.session);
  }

  function isRowPreviewing(row: SidebarRow): boolean {
    if (row.kind === "session") return row.session.id === previewSessionId;
    return row.linkedSession?.id === previewSessionId || (previewTaskWorkspace?.projectId === row.project.id && previewTaskWorkspace.taskKey === row.task.key);
  }

  function isRowFading(row: SidebarRow): boolean {
    const session = rowSession(row);
    return !!session && fadingSessionIds.has(session.id);
  }

  function runRowAction(action: RowAction, row: SidebarRow) {
    const session = rowSession(row);
    if (action === "review" && session) { onSelectSession(session.id); onToggleDiff?.(); }
    else if (action === "restart" && session) onRestartSession(session);
    else if (action === "done" && row.kind === "task") moveTask(row.task, row.project, "done");
    else if (action === "archive" && row.kind === "session") fadeOutThenAct(row.session.id, () => onArchiveSession(row.session));
    else if (action === "start" && row.kind === "task") onPickTask(row.task, row.project.path);
    else if (action === "reopen" && row.kind === "task") moveTask(row.task, row.project, "todo");
    else if (action === "wake" && row.kind === "task") void wakeTask(row.task, row.project);
    else if (action === "seen" && row.kind === "task") void taskStore.markTaskVisited(row.project.id, row.task.key).catch(reportUnreadFailure);
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

  async function moveTask(task: TaskItem, project: Project, status: string, followSelection = false) {
    const selectedBefore = getSelectedIndex();
    if (!(await changeTaskStatus(task.key, status, project.path, () => onSessionsChanged?.()))) return;
    // Only follow when the user has not moved the selection while the move was in flight.
    if (followSelection && getSelectedIndex() === selectedBefore) selectTaskOrItsGroup(project.id, task.key);
  }

  // Dragging a task row: to a lane for a status change, or to a session pane for a reference.
  function pressRow(e: PointerEvent, row: SidebarRow) {
    if (row.kind === "task") pressTaskRow(taskDragPayload(row, now), e, dropTask);
  }

  function dropTask(dragged: TaskDragPayload, target: TaskDropTarget) {
    if (target.kind === "pane") { onReferenceTask?.(dragged, target.paneId); return; }
    // The task as the store has it now decides: the payload is a snapshot from the press.
    const task = taskStore.findTask(dragged.repoPath, dragged.key);
    const project = projects.find((candidate) => candidate.id === dragged.projectId);
    if (!task || !project) return;
    const verdict = laneVerdict(taskDragPayload({ kind: "task", task, project, linkedSession: undefined }, now), target);
    if (!verdict.accepts) { showToast({ kind: "info", message: verdict.reason, autoDismissMs: 3000 }); return; }
    const { outcome } = verdict;
    if (outcome.kind === "status") moveTask(task, project, outcome.status);
    else if (outcome.kind === "wake") void wakeTask(task, project);
    else if (outcome.kind === "wake_status") void wakeAndChangeTaskStatus(task, project, outcome.status, () => onSessionsChanged?.());
    else if (outcome.kind === "snooze") snoozeOrWake({ task, project });
  }

  // The cursor says no over a lane or pane that refuses the dragged task.
  $effect(() => {
    document.body.classList.toggle("task-drop-refused", taskDrag.verdict?.accepts === false);
    return () => document.body.classList.remove("task-drop-refused");
  });

  const laneClass = (verdict: ReturnType<typeof laneHover>) =>
    verdict === null ? "" : verdict.accepts ? "task-lane-hovered rounded-lg bg-accent-bg ring-1 ring-inset ring-accent/40" : "task-lane-hovered rounded-lg ring-1 ring-inset ring-status-exited/40";
  // Rows settle into their new order after a move; a moved row glides to its new group, and a row
  // without a counterpart fades and slides in or out.
  const motionMs = () => (window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 150);
  const [send, receive] = crossfade({
    duration: motionMs,
    easing: cubicOut,
    fallback: () => ({ duration: motionMs(), easing: cubicOut, css: (t) => `opacity: ${t}; transform: translateY(${(t - 1) * 8}px)` }),
  });
  const emptyGroup = (key: string, lane: TaskLane): SidebarLaneGroup => ({ key, lane, rows: [], defaultCollapsed: true });
  // While a task is dragged, its project shows every lane group, and the status layout every status section.
  function projectGroups(section: SidebarProjectSection): SidebarLaneGroup[] {
    if (taskDrag.payload?.projectId !== section.project.id) return section.groups;
    return everyLane(section.groups, PROJECT_GROUP_LANES, (lane) => emptyGroup(`project-status:${section.project.id}:${lane}`, lane));
  }
  const classicSections = $derived.by((): SidebarSection[] => {
    if (!taskDrag.payload || groupBy !== "status") return sections;
    const status = sections.filter((section): section is Extract<SidebarSection, { kind: "status" }> => section.kind === "status");
    return [
      ...sections.filter((section) => section.kind === "sessions"),
      ...everyLane(status.map((section) => section.group), STATUS_SECTION_LANES, (lane) => emptyGroup(`status:${lane}`, lane))
        .map((group) => status.find((section) => section.group === group) ?? { kind: "status" as const, key: group.key, group, defaultCollapsed: true }),
    ];
  });

  /** Read and unread only mean something once an agent has done something on the task. */
  function unreadToggle(row: SidebarTaskRow): { label: string; onSelect: () => void } | null {
    if (row.task.last_activity_at === null || row.task.status === "done") return null;
    return isTaskUnread(row.task)
      ? { label: "Mark as read", onSelect: () => void taskStore.markTaskVisited(row.project.id, row.task.key).catch(reportUnreadFailure) }
      : { label: "Mark as unread", onSelect: () => void taskStore.markTaskUnread(row.project.id, row.task.key).catch(reportUnreadFailure) };
  }
  function toggleUnread(row: SidebarTaskRow) { unreadToggle(row)?.onSelect(); }
  function reportUnreadFailure(error: unknown) { showToast(`Failed to update the task: ${error}`, "error"); }

  // After a status change the task lives in another group; keep the keyboard selection on it,
  // or on its group header when that group is collapsed.
  function selectTaskOrItsGroup(projectId: string, taskKey: string) {
    for (const key of [navKey.task(projectId, taskKey), ...taskHeaderNavKeys(sections, projectId, taskKey)]) {
      const idx = flatNavIndex.get(key);
      if (idx !== undefined) { setSelectedIndex(idx); return; }
    }
  }

  // Flat nav list for keyboard navigation
  type NavItem = SidebarNavItem | { type: "plugin"; contributionKey: string; row: PluginSidebarNavRow };
  const flatNav = $derived.by(() => {
    const result: NavItem[] = sidebarNavItems(sections, isCollapsed, shownPages, isOpenRow);
    for (const contribution of activePluginContributions.filter((item) => item.contribution.placement === "sidebar.section")) {
      const contributionKey = `${contribution.plugin.id}:${contribution.contribution.id}`;
      for (const row of getPluginSidebarRows(contributionKey)) {
        result.push({ type: "plugin", contributionKey, row });
      }
    }
    return result;
  });

  // O(1) lookup map for nav index (avoids O(n²) findIndex in template)
  const flatNavIndex = $derived.by(() => {
    const map = new Map<string, number>();
    flatNav.forEach((item, i) => {
      map.set(item.type === "plugin" ? `plugin:${item.contributionKey}:${item.row.id}` : sidebarNavItemKey(item), i);
    });
    return map;
  });

  function selectPluginSidebarRow(contributionKey: string, rowId: string): void {
    const index = flatNavIndex.get(`plugin:${contributionKey}:${rowId}`);
    if (index !== undefined) setSelectedIndex(index);
  }

  $effect(() => {
    if (!navRef) return;
    const handlePluginSidebarSelect = (event: Event) => {
      const contributionKey = (event.target as HTMLElement | null)?.dataset.pluginUiContribution;
      const rowId = (event as CustomEvent<{ rowId?: unknown }>).detail?.rowId;
      if (contributionKey && typeof rowId === "string") selectPluginSidebarRow(contributionKey, rowId);
    };
    const handlePluginSidebarKeydown = (event: Event) => {
      const detail = (event as CustomEvent<{ event?: unknown; handled?: boolean }>).detail;
      if (!(detail?.event instanceof KeyboardEvent)) return;
      handleKeydown(detail.event);
      detail.handled = detail.event.defaultPrevented;
    };
    navRef.addEventListener("plugin-sidebar-select", handlePluginSidebarSelect);
    navRef.addEventListener("plugin-sidebar-keydown", handlePluginSidebarKeydown);
    return () => {
      navRef?.removeEventListener("plugin-sidebar-select", handlePluginSidebarSelect);
      navRef?.removeEventListener("plugin-sidebar-keydown", handlePluginSidebarKeydown);
    };
  });

  $effect(() => { clampIndex(flatNav.length); });


  // Scroll selected item into view on keyboard navigation
  $effect(() => {
    const idx = getSelectedIndex();
    if (skipNextAutoFocus || zone !== "sidebar" || !navRef) return;
    navRef.querySelector(`[data-nav-index="${idx}"]`)?.scrollIntoView({ block: "nearest" });
  });

  function releasePluginFrameFocus(): void {
    const active = deepActiveElement(document);
    if (active instanceof HTMLIFrameElement && navRef?.contains(document.activeElement)) releaseFrameFocus(active);
  }

  let focusedPluginRow = "";
  $effect(() => {
    const current = zone === "sidebar" ? flatNav[getSelectedIndex()] : undefined;
    // Keys pressed inside a plugin frame never reach this window, so a frame the selection left
    // must give the keyboard back, or Escape and the other sidebar keys stop working.
    if (current?.type !== "plugin") releasePluginFrameFocus();
    const key = current?.type === "plugin" ? `${current.contributionKey}:${current.row.id}` : "";
    if (key === focusedPluginRow) return;
    for (const item of flatNav) {
      if (item.type === "plugin" && `${item.contributionKey}:${item.row.id}` === focusedPluginRow) item.row.onFocus?.(false);
    }
    focusedPluginRow = key;
    if (current?.type === "plugin") current.row.onFocus?.(true);
  });

  // Auto-focus active session when sessions panel is toggled or its target changes.
  // A pointerdown precedes a row click. Do not scroll the active row between
  // those events or the release can target a different row.
  let autoFocusedSidebarTarget = "";
  let skipNextAutoFocus = $state(false);

  function handleSidebarPointerDown(): void {
    skipNextAutoFocus = true;
    focusSidebar();
  }

  $effect(() => {
    if (zone !== "sidebar" || getSidebarSubZone() !== "sessions") {
      autoFocusedSidebarTarget = "";
      return;
    }
    if (skipNextAutoFocus) {
      skipNextAutoFocus = false;
      return;
    }

    let target: string | undefined;
    if (activeSessionId) {
      target = flatNavIndex.has(navKey.orphan(activeSessionId))
          ? navKey.orphan(activeSessionId)
          : (() => {
              const active = sessions.find((session) => session.id === activeSessionId);
              return active?.task_key ? navKey.task(sessionTaskProjectId(active), active.task_key) : undefined;
            })();
    }

    if (!target || target === autoFocusedSidebarTarget) return;
    const idx = flatNavIndex.get(target);
    if (idx === undefined) return;
    setSelectedIndex(idx);
    autoFocusedSidebarTarget = target;
  });

  function handleKeydown(e: KeyboardEvent) {
    if (zone !== "sidebar") return;
    if (flatNav.length === 0) return;
    const origin = e.composedPath().find((node): node is Element => node instanceof Element);
    if (shouldBypassSidebarKeyboard(origin ?? document.activeElement)) return;
    const control = focusedSidebarControl(origin ?? document.activeElement);
    if (activatesItselfOnEnter(e, control)) return;

    const action = handleSidebarKey(e, flatNav.length);
    // Once the sidebar takes a key, such as a highlight move, the next Enter belongs to the highlighted row.
    if (e.defaultPrevented) control?.blur();
    if (!action) return;

    const idx = getSelectedIndex();
    const current = flatNav[idx];
    if (!current) return;

    if (action.type === "collapse") {
      if (current.type === "project_header") {
        setCollapsed(current.section.key, true, current.section.defaultCollapsed);
      } else if (current.type === "group_header") {
        if (!isCollapsed(current.key, current.defaultCollapsed)) {
          setCollapsed(current.key, true, current.defaultCollapsed);
        } else if (current.parentKey) {
          // Already collapsed group → jump to project header
          const parentIdx = flatNavIndex.get(navKey.project(current.parentKey));
          if (parentIdx !== undefined) setSelectedIndex(parentIdx);
        }
      } else if (current.type === "task" || current.type === "orphan" || current.type === "show_more") {
        // Jump to parent group header or project header
        for (let i = idx - 1; i >= 0; i--) {
          if (flatNav[i].type === "group_header" || flatNav[i].type === "project_header") {
            setSelectedIndex(i);
            break;
          }
        }
      } else if (current.type === "plugin") {
        current.row.onCollapse?.();
      }
      return;
    }

    if (action.type === "expand") {
      if (current.type === "project_header") {
        setCollapsed(current.section.key, false, current.section.defaultCollapsed);
      } else if (current.type === "group_header") {
        setCollapsed(current.key, false, current.defaultCollapsed);
      } else if (current.type === "plugin") {
        current.row.onExpand?.();
      }
      return;
    }

    if (current.type === "project_header") {
      if (action.type === "select") {
        toggleSection(current.section.key, current.section.defaultCollapsed);
      } else if (action.type === "edit") {
        onEditProject(current.section.project);
      }
      return;
    }

    if (current.type === "group_header") {
      if (action.type === "select") toggleSection(current.key, current.defaultCollapsed);
      return;
    }

    if (current.type === "show_more") {
      if (action.type === "select") showMore(current.shelfKey);
      return;
    }

    if (current.type === "orphan") {
      const session = current.session;
      if (action.type === "select") { onSelectSession(session.id); focusTerminal(); }
      else if (action.type === "archive") fadeOutThenAct(session.id, () => onArchiveSession(session));
      else if (action.type === "delete") fadeOutThenAct(session.id, () => onDeleteSession(session));
      else if (action.type === "rename") startRename(session);
      else if (action.type === "restart") onRestartSession(session);
      else if (action.type === "review") { onSelectSession(session.id); onToggleDiff?.(); }
    } else if (current.type === "task") {
      const { task, project, linkedSession: linked } = current.row;
      if (action.type === "select") handleTaskClick(task, project);
      else if (action.type === "start_session") onPickTask(task, project.path);
      else if (action.type === "edit") onEditTask(task, project);
      else if (action.type === "status") moveTask(task, project, action.status, true);
      else if (action.type === "review") { if (linked) { onSelectSession(linked.id); onToggleDiff?.(); } }
      else if (action.type === "archive") { if (linked) fadeOutThenAct(linked.id, () => onArchiveSession(linked)); }
      else if (action.type === "delete") { if (linked) onDeleteSession(linked); }
      else if (action.type === "restart") { if (linked) onRestartSession(linked); }
      else if (action.type === "toggle_unread") toggleUnread(current.row);
      else if (action.type === "snooze") snoozeOrWake({ task, project });
    } else if (current.type === "plugin" && action.type === "select") {
      current.row.onSelect?.();
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


{#snippet projectLabel(project: Project)}
  {#if showProjectLabels}<span class="shrink-0 max-w-[40%] truncate text-[10px] text-t3" title={project.path}>{project.name}</span>{/if}
{/snippet}

{#snippet sectionHeader(navIdx: number, collapsed: boolean, onToggle: () => void, label: string, count: number, extras: { dotClass?: string | null; title?: string; oncontextmenu?: (e: MouseEvent) => void; autoDispatch?: boolean; leadingSpace?: boolean; drop?: ReturnType<typeof laneHover> })}
  <button
    data-nav-index={navIdx}
    class="w-full {extras.leadingSpace ? 'pl-6 pr-2' : 'px-2'} mb-1 text-[11px] font-semibold text-t2 uppercase tracking-[.05em] truncate flex items-center gap-1.5 rounded-lg py-1 hover:bg-panel-hi {zone === 'sidebar' && navIdx === getSelectedIndex() ? 'ring-2 ring-accent' : ''}"
    title={extras.title}
    aria-expanded={!collapsed}
    onclick={onToggle}
    oncontextmenu={extras.oncontextmenu}
  >
    {#if extras.dotClass}<span class="size-1.5 shrink-0 rounded-full {extras.dotClass}"></span>{/if}
    {label}
    {#if extras.drop}<LaneDropBadge verdict={extras.drop} />{/if}
    <span class="ml-auto font-normal text-t3">{count}</span>
    {#if extras.autoDispatch}<Zap class="size-2.5 text-status-running" />{/if}
    {#if collapsed}<ChevronRight class="size-3 shrink-0 text-t3" />{:else}<ChevronDown class="size-3 shrink-0 text-t3" />{/if}
  </button>
{/snippet}

{#snippet groupHeader(key: string, defaultCollapsed: boolean, label: string, count: number, dotClass: string | null, level: "top" | "sub", drop: ReturnType<typeof laneHover> = null)}
  {@const navIdx = flatNavIndex.get(navKey.group(key)) ?? -1}
  {@const collapsed = isCollapsed(key, defaultCollapsed)}
  {#if level === "top"}
    {@render sectionHeader(navIdx, collapsed, () => toggleSection(key, defaultCollapsed), label, count, { dotClass, drop })}
  {:else}
    <button
      data-nav-index={navIdx}
      class="w-full flex items-center gap-1.5 pl-2 pr-2 py-1 text-[9.5px] font-semibold text-t2 uppercase tracking-[.05em] hover:opacity-80 rounded-lg {zone === 'sidebar' && navIdx === getSelectedIndex() ? 'ring-2 ring-accent' : ''}"
      aria-expanded={!collapsed}
      onclick={() => toggleSection(key, defaultCollapsed)}
    >
      {#if dotClass}<span class="size-1.5 rounded-full {dotClass}"></span>{/if}
      {label}
      <span class="font-normal text-t3 ml-0.5">{count}</span>
      {#if drop}<LaneDropBadge verdict={drop} />{/if}
      {#if collapsed}<ChevronRight class="size-3 ml-auto text-t3" />{:else}<ChevronDown class="size-3 ml-auto text-t3" />{/if}
    </button>
  {/if}
{/snippet}

<!-- A lane group of the Classic layout, which takes a dragged task of its project (every project when `projectId` is null). -->
{#snippet laneGroup(group: SidebarLaneGroup, projectId: string | null, level: "top" | "sub")}
  {@const drop = laneHover(group.lane, projectId)}
  <div class={laneClass(drop)} data-task-lane={group.lane} data-task-lane-project={projectId ?? undefined}>
    {@render groupHeader(group.key, group.defaultCollapsed, LANES[group.lane].label, group.rows.length, LANES[group.lane].dotClass, level, drop)}
    {#if !isCollapsed(group.key, group.defaultCollapsed)}{@render taskList(group.rows)}{/if}
  </div>
{/snippet}

{#snippet activityIndicator(activity: RowActivity | null, row: SidebarRow)}
  {#if activity === "snoozed"}
    <SnoozeCountdown snoozedUntil={row.kind === "task" ? row.task.snoozed_until : null} class="font-mono text-[10px]" />
  {:else if activity === "woke"}
    <span class="flex" role="img" aria-label="Woke" title={WOKE_TITLE}><AlarmClock class="size-3.5 text-status-review" /></span>
  {:else if activity === "working"}
    <LoaderCircle class="size-3 animate-spin text-t2" />
  {:else if activity === "unread"}
    <span class="flex" title="Unread: the agent finished since you last opened this {row.kind}"><Lightbulb class="size-3.5 text-status-review animate-pulse" /></span>
  {:else if activity === "exited"}
    <span class="font-mono text-[9px] text-t3 bg-panel-hi rounded px-[5px] py-[1px]">exited</span>
  {/if}
{/snippet}

{#snippet orphanList(orphans: SidebarSessionRow[])}
  {#if orphans.length > 0}
    <ul class="space-y-0.5 mb-1">
      {#each orphans as { session, project } (session.id)}
        {@const globalIndex = flatNavIndex.get(navKey.orphan(session.id)) ?? -1}
        {@const isActive = session.id === activeSessionId}
        {@const isSelected = zone === 'sidebar' && globalIndex === getSelectedIndex()}
        {@const isPreviewing = session.id === previewSessionId}
        {@const orphanRow: SidebarRow = { kind: "session", session, project }}
        {@const hint = jumpHintFor(session.id)}
        <li class="transition-opacity duration-200 {fadingSessionIds.has(session.id) ? 'opacity-0' : 'opacity-100'}">
          {#if renamingSessionId === session.id}
            <SessionRenameInput bind:value={renameValue} onCommit={() => commitRename(session.id)} onCancel={() => onStartRename("")} />
          {:else}
            <div class="session-row relative flex items-center gap-1.5">
              <span class="w-[2px] self-stretch rounded-full transition-opacity {isActive ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
              <button
                data-nav-index={globalIndex}
                class="flex-1 min-w-0 text-left py-[6px] text-[13px] flex items-center gap-1.5 transition-colors rounded-lg pl-4 pr-2
                  {isActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
                  {isPreviewing ? 'ring-2 ring-accent' : isSelected ? 'ring-2 ring-accent' : ''}"
                onclick={() => handleOrphanClick(session)}
                oncontextmenu={(e) => onContextMenu(e, session)}
              >
              {@render projectLabel(project)}
              <span class="truncate font-medium text-t1">{session.name || session.branch}</span>
              <span class="ml-auto shrink-0 flex items-center gap-1.5">
                {#if hint}<JumpHintBadge label={hint} />{:else}{@render activityIndicator(rowActivity(orphanRow, agentStates, unreadSessions, now), orphanRow)}{/if}
              </span>
            </button>
            {#each sessionIndicatorContributions as item (`${session.id}:${item.plugin.id}:${item.contribution.id}`)}
              <div class="absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 pointer-events-none {hint ? 'hidden' : ''}" data-plugin-session-indicator={session.id}>
                <PluginContributionHost plugin={item.plugin} contribution={item.contribution} session={pluginSessionContext(session)} onNavigate={onPluginNavigate ?? (() => {})} onClose={onPluginClose ?? (() => {})} />
              </div>
            {/each}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet taskList(rows: SidebarTaskRow[])}
  <ul class="space-y-0.5">
    {#each rows as row (`${row.project.id}:${row.task.key}`)}
      {@const { task, project, linkedSession: linked } = row}
      {@const activity = rowActivity(row, agentStates, unreadSessions, now)}
      {@const isActive = isOpenRow(row)}
      {@const taskNavIdx = flatNavIndex.get(navKey.task(project.id, task.key)) ?? -1}
      {@const isSelected = zone === 'sidebar' && taskNavIdx === getSelectedIndex()}
      {@const isPreviewing = (linked?.id === previewSessionId) || (previewTaskWorkspace?.projectId === project.id && previewTaskWorkspace.taskKey === task.key)}
      {@const hint = jumpHintFor(navKey.task(project.id, task.key))}
      {@const key = navKey.task(project.id, task.key)}
      <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key }} out:send={{ key }} class="transition-opacity duration-200 {linked && fadingSessionIds.has(linked.id) ? 'opacity-0' : 'opacity-100'}">
        <div class="session-row relative flex items-center gap-1.5" {@attach liftWhileDragged({ projectId: project.id, key: task.key })}>
          <span class="w-[2px] self-stretch rounded-full transition-opacity {isActive ? 'bg-accent opacity-100' : 'opacity-0'}"></span>
          <button
            data-nav-index={taskNavIdx}
            data-snooze-anchor={snoozeAnchor(project.id, task.key)}
            class="flex-1 min-w-0 text-left py-[6px] pl-4 pr-2 flex items-center gap-1.5 transition-colors rounded-lg
              {isActive ? 'bg-accent-bg' : 'hover:bg-panel-hi'}
              {isPreviewing ? 'ring-2 ring-accent' : isSelected ? 'ring-2 ring-accent' : ''}"
            title={showTaskKeys ? undefined : task.key}
            onclick={() => handleTaskClick(task, project)}
            onpointerdown={(e) => pressRow(e, row)}
            oncontextmenu={(e) => onTaskContextMenu(e, row)}
          >
          {@render projectLabel(project)}
          <!-- The parent prefix is dropped next to a project label to keep at most two small labels before the title. -->
          {#if showTaskKeys && task.parent_key && !showProjectLabels}<span class="shrink-0 font-mono text-[10px] text-t3">{task.parent_key} ›</span>{/if}
          {#if showTaskKeys}<span class="shrink-0 font-mono text-[10px] {linked ? 'text-accent/70' : 'text-t3'}">{task.key}</span>{/if}
          <span class="truncate text-[12.5px] {task.status === 'done' ? 'line-through text-t3' : 'text-t1'}">{task.title}</span>
          {#if hint}
            <span class="ml-auto shrink-0 flex items-center"><JumpHintBadge label={hint} /></span>
          {:else if activity}
            <span class="ml-auto shrink-0 flex items-center gap-1.5">{@render activityIndicator(activity, row)}</span>
          {/if}
        </button>
        {#if linked}
          {#each sessionIndicatorContributions as item (`${linked.id}:${item.plugin.id}:${item.contribution.id}`)}
            <div class="absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 pointer-events-none {hint ? 'hidden' : ''}" data-plugin-session-indicator={linked.id}>
              <PluginContributionHost plugin={item.plugin} contribution={item.contribution} session={pluginSessionContext(linked)} onNavigate={onPluginNavigate ?? (() => {})} onClose={onPluginClose ?? (() => {})} />
            </div>
          {/each}
        {/if}
        </div>
      </li>
    {/each}
  </ul>
{/snippet}

<aside
  class="relative shrink-0 flex flex-col border-r bg-sidebar {zone === 'sidebar' ? 'border-accent' : 'border-border'}"
  style:width="{sidebarWidth}px"
  onpointerdown={handleSidebarPointerDown}
  onfocusin={focusSidebar}
>
  <ResizeHandle side="right" bind:width={sidebarWidth} min={160} max={Infinity} defaultWidth={266} onResizeEnd={(w) => setLayoutWidth("sidebar", w)} />

  <!-- Header: new task + new project buttons -->
  <div class="px-3 pt-3 pb-2 flex gap-1.5">
    {#if projects.length > 0}
      <button
        onclick={() => onCreateTask?.()}
        aria-label="New task"
        class="{headerBtnClass} flex-[3] justify-between px-3"
      >
        <span class="flex items-center gap-1.5 overflow-hidden">
          <Plus class="size-3.5 shrink-0" />
          {#if sidebarWidth >= 220}<span class="truncate">New task</span>{/if}
        </span>
        {#if sidebarWidth >= 220}<span class="font-mono text-[10px] text-t3 shrink-0 ml-1">{MOD_LABEL}N</span>{/if}
      </button>
    {/if}
    <button
      onclick={() => onAddProject?.()}
      aria-label="New project"
      title={compactProjectButton ? `New project (${MOD_LABEL}⇧N)` : undefined}
      class="{headerBtnClass} {compactProjectButton ? 'w-8 shrink-0 justify-center px-0' : `justify-between px-3 ${projects.length > 0 ? 'flex-[2]' : 'flex-[1]'}`}"
    >
      <span class="flex items-center gap-1.5 overflow-hidden">
        <FolderPlus class="size-3.5 shrink-0" />
        {#if !compactProjectButton}<span class="truncate">New project</span>{/if}
      </span>
      {#if !compactProjectButton}<span class="font-mono text-[10px] text-t3 shrink-0 ml-1">{MOD_LABEL}⇧N</span>{/if}
    </button>
  </div>

  {#each activePluginContributions.filter((item) => item.contribution.placement === "sidebar.header") as item (`${item.plugin.id}:${item.contribution.id}`)}
    <div class="px-3 pb-2" data-plugin-sidebar-slot="header"><PluginContributionHost plugin={item.plugin} contribution={item.contribution} onNavigate={onPluginNavigate ?? (() => {})} onClose={onPluginClose ?? (() => {})} onOpenPreferences={() => onOpenPreferences(pluginPreferencesLocation(item.plugin))} onFailure={() => markSidebarContributionFailed(item)} /></div>
  {/each}

  <!-- Main content -->
  <nav bind:this={navRef} data-task-drag-list class="flex-1 overflow-y-auto overflow-x-hidden px-2 py-2 space-y-3 scrollbar-hide select-none {layout === 'cards' ? 'flex flex-col' : ''}" oncontextmenu={onNavContextMenu}>
    {#if projects.length === 0}
      <div class="mt-12 text-center px-4 space-y-3">
        <p class="text-xs text-t3">No projects yet</p>
        <button onclick={onAddProject} class="text-xs text-accent hover:underline">Add a project →</button>
      </div>
    {:else if layout === "cards"}
      <SidebarCardsList
        {sections}
        {showTaskKeys}
        showProjects={showProjectLabels}
        navIndex={(key) => flatNavIndex.get(key) ?? -1}
        {isCollapsed}
        onToggle={toggleSection}
        {shownPages}
        onShowMore={showMore}
        isActive={isOpenRow}
        isPreviewing={isRowPreviewing}
        isFading={isRowFading}
        rename={{
          sessionId: renamingSessionId,
          value: renameValue,
          onInput: (value) => (renameValue = value),
          onCommit: commitRename,
          onCancel: () => onStartRename(""),
        }}
        {projectAutoMode}
        onSelectRow={selectRow}
        onRowPress={pressRow}
        onRowAction={runRowAction}
        {onRowContextMenu}
        {onProjectContextMenu}
        plugins={{
          indicators: sessionIndicatorContributions,
          meta: sessionMetaContributions,
          sessionContext: pluginSessionContext,
          onNavigate: onPluginNavigate,
          onClose: onPluginClose,
          onOpenPanel: onOpenPluginSessionPanel,
        }}
      />
    {:else}
      {#each classicSections as section (section.key)}
        {#if section.kind === "project"}
          {@const project = section.project}
          {@const projectCollapsed = isCollapsed(section.key, section.defaultCollapsed)}
          {@const projectNavIdx = flatNavIndex.get(navKey.project(section.key)) ?? -1}
          <div class="relative">
            {@render sectionHeader(projectNavIdx, projectCollapsed, () => toggleSection(section.key, section.defaultCollapsed), project.name, section.orphans.length + section.taskCount, {
              title: project.path,
              oncontextmenu: (e) => onProjectContextMenu(e, project),
              autoDispatch: projectAutoMode[project.id],
              leadingSpace: project.path_missing,
            })}
            {#if project.path_missing}
              <button
                type="button"
                class="absolute left-1 top-[3px] size-[18px] flex items-center justify-center rounded text-status-review hover:bg-panel-hi"
                title={`Folder not found: ${project.path}`}
                aria-label={`Locate folder for ${project.name}. Folder not found: ${project.path}`}
                data-testid="project-folder-missing"
                onclick={() => locateProjectFolder(project)}
              ><TriangleAlert class="size-3" /></button>
            {/if}

            {#if !projectCollapsed}
              {@render orphanList(section.orphans)}
              {#each projectGroups(section) as group (group.key)}
                {@render laneGroup(group, project.id, "sub")}
              {/each}
              {#if section.orphans.length === 0 && section.taskCount === 0}
                <p class="px-3 py-1 text-xs text-t3 italic">No sessions or tasks</p>
              {/if}
            {/if}
          </div>
        {:else if section.kind === "sessions"}
          <div>
            {@render groupHeader(section.key, section.defaultCollapsed, "Sessions", section.orphans.length, null, "top")}
            {#if !isCollapsed(section.key, section.defaultCollapsed)}
              {@render orphanList(section.orphans)}
            {/if}
          </div>
        {:else if section.kind === "status"}
          {@render laneGroup(section.group, null, "top")}
        {/if}
      {:else}
        {#if groupBy === "status"}
          <p class="px-3 py-1 text-xs text-t3 italic">No sessions or tasks</p>
        {/if}
      {/each}

    {/if}

    {#each activePluginContributions.filter((item) => item.contribution.placement === "sidebar.section") as item (`${item.plugin.id}:${item.contribution.id}`)}
      <div data-plugin-sidebar-slot="section">
        <PluginContributionHost plugin={item.plugin} contribution={item.contribution} onNavigate={onPluginNavigate ?? (() => {})} onClose={onPluginClose ?? (() => {})} onOpenPreferences={() => onOpenPreferences(pluginPreferencesLocation(item.plugin))} onFailure={() => markSidebarContributionFailed(item)} />
      </div>
    {/each}

    {#each activePluginContributions.filter((item) => item.contribution.placement === "sidebar.navigation") as item (`${item.plugin.id}:${item.contribution.id}`)}
      <div data-plugin-sidebar-slot="navigation"><PluginContributionHost plugin={item.plugin} contribution={item.contribution} onNavigate={onPluginNavigate ?? (() => {})} onClose={onPluginClose ?? (() => {})} onOpenPreferences={() => onOpenPreferences(pluginPreferencesLocation(item.plugin))} onFailure={() => markSidebarContributionFailed(item)} /></div>
    {/each}
  </nav>

  {#each activePluginContributions.filter((item) => item.contribution.placement === "sidebar.footer") as item (`${item.plugin.id}:${item.contribution.id}`)}
    <div class="border-t border-border px-2 py-2" data-plugin-sidebar-slot="footer"><PluginContributionHost plugin={item.plugin} contribution={item.contribution} onNavigate={onPluginNavigate ?? (() => {})} onClose={onPluginClose ?? (() => {})} onOpenPreferences={() => onOpenPreferences(pluginPreferencesLocation(item.plugin))} onFailure={() => markSidebarContributionFailed(item)} /></div>
  {/each}

  <!-- Preferences footer -->
  <button
    onclick={() => onOpenPreferences()}
    class="flex items-center gap-2 px-3 py-2.5 border-t border-border text-t2 text-[12px] hover:bg-panel-hi transition-colors"
  >
    <Settings class="size-3.5" />
    <span>Preferences</span>
    <span class="ml-auto font-mono text-[10px] text-t3">{MOD_LABEL},</span>
  </button>
</aside>

<!-- Session context menu -->
{#if contextMenu}
  {@const menuSession = contextMenu.session}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    onClose={() => (contextMenu = null)}
    items={[
      ...(pluginSessionActionsForProvider(pluginSessionActions, menuSession.provider).length > 0
        ? [{
            label: "Integration actions",
            children: pluginSessionActionsForProvider(pluginSessionActions, menuSession.provider).map((action) => ({
              label: action.label,
              onSelect: () => onPluginSessionAction?.(menuSession, action),
            })),
          }]
        : []),
      ...(menuSession.status === 'exited'
        ? [
            { label: "Restart", onSelect: () => onRestartSession(menuSession) },
            { label: "Rename", onSelect: () => startRename(menuSession) },
            { label: "Archive", onSelect: () => fadeOutThenAct(menuSession.id, () => onArchiveSession(menuSession)) },
            { label: "Delete", danger: true, onSelect: () => fadeOutThenAct(menuSession.id, () => onDeleteSession(menuSession)) },
          ]
        : [
            { label: "Review", onSelect: () => { onSelectSession(menuSession.id); onToggleDiff?.(); } },
            { label: "Rename", onSelect: () => startRename(menuSession) },
            { label: "Archive", onSelect: () => fadeOutThenAct(menuSession.id, () => onArchiveSession(menuSession)) },
            { label: "Delete", danger: true, onSelect: () => fadeOutThenAct(menuSession.id, () => onDeleteSession(menuSession)) },
          ]),
    ]}
  />
{/if}

<style>
  :global(.session-row:has([data-plugin-indicator-visible="true"]):not(:has([data-jump-hint])) > button) {
    padding-right: 1.75rem;
  }
</style>

<!-- Project context menu -->
{#if projectContextMenu}
  <ContextMenu
    x={projectContextMenu.x}
    y={projectContextMenu.y}
    onClose={() => (projectContextMenu = null)}
    items={projectContextMenuItems(
      projectContextMenu.project,
      projectAutoMode[projectContextMenu.project.id] ?? false,
      {
        onEdit: onEditProject,
        onLocate: locateProjectFolder,
        onToggleAutoDispatch: toggleAutoMode,
        onHide: hideProject,
        onArchive: (id) => projectStore.archiveProject(id),
        onDelete: onDeleteProject,
      },
    )}
  />
{/if}

<!-- Task context menu -->
{#if taskContextMenu}
  {@const menuRow = taskContextMenu.row}
  {@const menuTask = menuRow.task}
  {@const linkedSession = menuRow.linkedSession}
  {@const statusChildren = STATUS_OPTIONS
    .filter(s => s.value !== menuTask.status)
    .map(s => ({ label: s.label, onSelect: () => moveTask(menuTask, menuRow.project, s.value) }))}
  {@const readToggle = unreadToggle(menuRow)}
  {@const snoozeEntry = snoozeMenuEntry({ task: menuTask, project: menuRow.project })}
  <ContextMenu
    x={taskContextMenu.x}
    y={taskContextMenu.y}
    onClose={() => (taskContextMenu = null)}
    items={[
      ...(linkedSession
        ? [{ label: "Review diff", onSelect: () => { onSelectSession(linkedSession.id); onToggleDiff?.(); } }]
        : []),
      ...(readToggle ? [readToggle] : []),
      { label: "Edit task", onSelect: () => onEditTask(menuTask, menuRow.project) },
      { label: "Change status", children: statusChildren },
      ...(snoozeEntry ? [snoozeEntry] : []),
      ...(linkedSession
        ? [
            ...(linkedSession.status === 'exited' ? [{ label: "Restart session", onSelect: () => onRestartSession(linkedSession) }] : []),
            { label: "Archive session", onSelect: () => fadeOutThenAct(linkedSession.id, () => onArchiveSession(linkedSession)) },
            { label: "Delete session", danger: true, onSelect: () => onDeleteSession(linkedSession) },
          ]
        : []),
    ]}
  />
{/if}

<!-- Sidebar view options menu -->
{#if viewMenu}
  <ContextMenu
    x={viewMenu.x}
    y={viewMenu.y}
    onClose={() => (viewMenu = null)}
    items={sidebarViewMenuItems(getSettings(), (patch) => updateSettings(patch).catch((error) => showToast(`Failed to save settings: ${error}`, "error")))}
  />
{/if}
