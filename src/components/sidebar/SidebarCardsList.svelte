<script lang="ts">
  import { Tooltip } from "bits-ui";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { crossfade } from "svelte/transition";
  import { AlarmClock, AlarmClockOff, Check, ChevronDown, CircleAlert, CircleCheck, CircleDashed, GitBranch, GitCompare, Play, RotateCcw, TriangleAlert, Undo2, Zap, type LucideIcon } from "@lucide/svelte";
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import JumpHintBadge from "./JumpHintBadge.svelte";
  import SessionMetaSlot from "./SessionMetaSlot.svelte";
  import SessionRenameInput from "./SessionRenameInput.svelte";
  import { getActiveZone } from "../../lib/focus.svelte";
  import { jumpHintFor } from "../../lib/jump-hints.svelte";
  import { getSelectedIndex } from "../../lib/sidebar-nav.svelte";
  import { locateProjectFolder } from "../../lib/locate-project";
  import * as orchestrator from "../../lib/session-orchestrator.svelte";
  import { everyLane, LANES, SHELVES, navKey, shelfPage, sidebarRowNavKey, sidebarRowTargetId, type IsCollapsed, type ShelfLane, type ShownPages, type SidebarFolderSection, type SidebarRow, type SidebarSection, type SidebarShelfSection } from "../../lib/sidebar-model";
  import { folderUrgency, isQuiet, rowActions, rowActivity, rowSession, splitBranch, WOKE_TITLE, type FolderUrgency, type RowAction, type RowActivity } from "../../lib/sidebar-cards";
  import { snoozeAnchor } from "../../lib/snooze-picker.svelte";
  import { getLaneNow } from "../../lib/snooze-clock.svelte";
  import { laneHover, liftWhileDragged, taskDrag } from "../../lib/task-drag.svelte";
  import LaneDropBadge from "./LaneDropBadge.svelte";
  import SnoozeCountdown from "./SnoozeCountdown.svelte";
  import type { PluginSessionContext } from "../../lib/plugin-sdk";
  import type { PluginSlot, Project, Session } from "../../lib/types";

  interface Props {
    sections: SidebarSection[];
    showTaskKeys: boolean;
    /** Whether cards outside a project folder show their project line. */
    showProjects: boolean;
    navIndex: (key: string) => number;
    isCollapsed: IsCollapsed;
    onToggle: (key: string, fallback: boolean) => void;
    shownPages: ShownPages;
    onShowMore: (shelfKey: string) => void;
    isActive: (row: SidebarRow) => boolean;
    isPreviewing: (row: SidebarRow) => boolean;
    isFading: (row: SidebarRow) => boolean;
    rename: {
      sessionId: string | null;
      value: string;
      onInput: (value: string) => void;
      onCommit: (sessionId: string) => void;
      onCancel: () => void;
    };
    projectAutoMode: Record<string, boolean>;
    onSelectRow: (row: SidebarRow) => void;
    /** A press on a row, which may become a drag. */
    onRowPress: (e: PointerEvent, row: SidebarRow) => void;
    onRowAction: (action: RowAction, row: SidebarRow) => void;
    onRowContextMenu: (e: MouseEvent, row: SidebarRow) => void;
    onProjectContextMenu: (e: MouseEvent, project: Project) => void;
    plugins: {
      indicators: PluginSlot[];
      /** Slots beside the branch line. */
      meta: PluginSlot[];
      sessionContext: (session: Session) => PluginSessionContext;
      onNavigate?: (pluginId: string, contributionId: string) => void;
      onClose?: () => void;
      /** Selects the session and opens the plugin's session panel, from its meta slot. */
      onOpenPanel?: (sessionId: string, pluginId: string, contributionId: string) => void;
    };
  }

  let {
    sections, showTaskKeys, showProjects, navIndex, isCollapsed, onToggle, shownPages, onShowMore,
    isActive, isPreviewing, isFading, rename, projectAutoMode, onSelectRow, onRowPress, onRowAction, onRowContextMenu, onProjectContextMenu, plugins,
  }: Props = $props();

  const ACTIVITY_SLOTS: Record<Exclude<RowActivity, "snoozed">, { icon: LucideIcon; label: string; class: string }> = {
    woke: { icon: AlarmClock, label: "Woke", class: "text-status-review" },
    working: { icon: CircleDashed, label: "Working", class: "text-status-working" },
    unread: { icon: CircleCheck, label: "Unread", class: "text-status-review" },
    exited: { icon: CircleAlert, label: "Exited", class: "text-status-exited" },
  };

  const ACTIONS: Record<RowAction, { icon: LucideIcon; label: string; text?: string; class?: string }> = {
    seen: { icon: AlarmClock, label: "Woke: mark as seen", text: "Woke", class: "text-status-review" },
    review: { icon: GitCompare, label: "Review diff" },
    restart: { icon: RotateCcw, label: "Restart session" },
    done: { icon: Check, label: "Mark done", text: "Done" },
    archive: { icon: Check, label: "Archive session", text: "Done" },
    start: { icon: Play, label: "Start session", text: "Start" },
    reopen: { icon: Undo2, label: "Reopen", text: "Reopen" },
    wake: { icon: AlarmClockOff, label: "Wake now", text: "Wake" },
  };

  const URGENCY_DOTS: Record<FolderUrgency, { class: string; label: string }> = {
    exited: { class: "bg-status-exited", label: "Has an exited session" },
    review: { class: "bg-status-review", label: "Needs review" },
    running: { class: "bg-status-working", label: "Working" },
  };

  const top = $derived(sections.filter((s) => s.kind === "attention" || s.kind === "folder"));
  const shelves = $derived(sections.filter((s): s is SidebarShelfSection => s.kind === "shelf"));
  // A dragged task can land in an empty lane, so every shelf shows while the drag lasts.
  const shelvesShown = $derived(
    taskDrag.payload
      ? everyLane(shelves, Object.keys(SHELVES) as ShelfLane[], (lane): SidebarShelfSection => ({ kind: "shelf", key: `shelf:${lane}`, lane, rows: [], defaultCollapsed: true }))
      : shelves,
  );
  const topDrop = $derived(laneHover("in_review", null));
  // Session rows never lift: an id no drag carries.
  const rowId = (row: SidebarRow) => row.kind === "task" ? { projectId: row.project.id, key: row.task.key } : { projectId: "", key: "" };
  const laneClass = (verdict: ReturnType<typeof laneHover>) =>
    verdict === null ? "" : verdict.accepts ? "rounded-md bg-accent-bg ring-1 ring-inset ring-accent/40" : "rounded-md ring-1 ring-inset ring-status-exited/40";
  const agentStates = $derived(orchestrator.getAgentStates());
  const unreadSessions = $derived(orchestrator.getUnreadSessions());

  const now = $derived(getLaneNow());
  const activityOf = (row: SidebarRow) => rowActivity(row, agentStates, unreadSessions, now);
  // The status slot gives way to hover actions, so the row itself explains an unread.
  function rowTitle(row: SidebarRow, activity: RowActivity | null): string | undefined {
    const lines = [
      row.kind === "task" && !showTaskKeys ? row.task.key : null,
      activity === "unread" ? `Unread: the agent finished since you last opened this ${row.kind}` : null,
      activity === "woke" ? WOKE_TITLE : null,
    ].filter((line) => line !== null);
    return lines.length > 0 ? lines.join("\n") : undefined;
  }
  const isSelected = (navIdx: number) => getActiveZone() === "sidebar" && navIdx === getSelectedIndex();

  const motionMs = () => (window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 150);
  // A row moving between lists glides from its old place; a row without a counterpart fades and slides in or out.
  const [send, receive] = crossfade({
    duration: motionMs,
    easing: cubicOut,
    fallback: () => ({ duration: motionMs(), easing: cubicOut, css: (t) => `opacity: ${t}; transform: translateY(${(t - 1) * 8}px)` }),
  });

  const rowItemClass = (row: SidebarRow) => `transition-opacity duration-200 ${isFading(row) ? "opacity-0" : "opacity-100"}`;

  function rowStateClass(active: boolean, selected: boolean, ringed: boolean): string {
    const background = active ? "bg-sidebar-row-active" : selected ? "bg-sidebar-row-selected" : "group-hover:bg-sidebar-row-hover";
    const ring = ringed ? "ring-2 ring-inset ring-accent" : active ? "ring-1 ring-inset ring-border-s" : "";
    return `${background} ${ring}`;
  }
</script>

{#snippet chevron(collapsed: boolean)}
  <ChevronDown class="size-3 shrink-0 text-t3 transition-transform {collapsed ? '-rotate-90' : ''}" />
{/snippet}

{#snippet pluginHost(item: PluginSlot, session: Session)}
  <PluginContributionHost plugin={item.plugin} contribution={item.contribution} session={plugins.sessionContext(session)} onNavigate={plugins.onNavigate ?? (() => {})} onClose={plugins.onClose ?? (() => {})} />
{/snippet}

{#snippet trailing(row: SidebarRow, session: Session | undefined, activity: RowActivity | null, hint: string | undefined)}
  {#if session}
    <!-- Hidden, not unmounted, so plugin indicators keep their state through a hint. -->
    {#each plugins.indicators as item (`${session.id}:${item.plugin.id}:${item.contribution.id}`)}
      <span class="size-4 shrink-0 has-[[data-plugin-indicator-visible=false]]:hidden {hint ? 'hidden' : 'flex'}" data-plugin-session-indicator={session.id}>
        {@render pluginHost(item, session)}
      </span>
    {/each}
  {/if}
  {#if hint}
    <JumpHintBadge label={hint} />
  {:else if activity === "snoozed"}
    <SnoozeCountdown snoozedUntil={row.kind === "task" ? row.task.snoozed_until : null} class="shrink-0 text-xs transition-opacity group-focus-within:opacity-0 group-hover:opacity-0" />
  {:else if activity}
    {@const slot = ACTIVITY_SLOTS[activity]}
    <span class="flex shrink-0 items-center gap-1 text-xs tabular-nums transition-opacity group-focus-within:opacity-0 group-hover:opacity-0 {slot.class}">
      <slot.icon class="size-3.5" />{slot.label}
    </span>
  {/if}
{/snippet}

{#snippet hoverActions(row: SidebarRow, shelf: ShelfLane | undefined, activity: RowActivity | null, active: boolean, position: string)}
  <div class="pointer-events-none absolute right-2.5 flex h-6 items-center rounded-md bg-sidebar opacity-0 transition-opacity group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100 {position}">
    <!-- Layered over an opaque base so the actions cover the row content with the row's own background. -->
    <div class="flex h-full items-center gap-0.5 rounded-md {active ? 'bg-sidebar-row-active' : 'bg-sidebar-row-hover'}">
      {#each rowActions(row, shelf, activity) as action (action)}
        {@const spec = ACTIONS[action]}
        <button
          type="button"
          class="flex items-center rounded-md hover:bg-sidebar-row-hover {spec.class ?? 'text-t2 hover:text-t1'} {spec.text ? 'h-6 gap-1 px-1.5 text-xs' : 'size-6 justify-center'}"
          aria-label={spec.label}
          title={spec.label}
          onclick={(e) => { e.stopPropagation(); onRowAction(action, row); }}
        ><spec.icon class="size-3.5 shrink-0" />{spec.text}</button>
      {/each}
    </div>
  </div>
{/snippet}

{#snippet titleParts(row: SidebarRow, receded: boolean)}
  {#if row.kind === "task"}
    {#if showTaskKeys && row.task.parent_key}<span class="shrink-0 font-mono text-[10px] text-t3">{row.task.parent_key} ›</span>{/if}
    {#if showTaskKeys}<span class="shrink-0 font-mono text-[10px] text-t3">{row.task.key}</span>{/if}
  {/if}
  <span class="min-w-0 flex-1 truncate text-[13px] text-t1 {receded ? 'font-normal group-hover:font-medium group-has-focus-visible:font-medium' : 'font-medium'}">{row.kind === "task" ? row.task.title : row.session.name || row.session.branch}</span>
{/snippet}

{#snippet card(row: SidebarRow, inFolder: boolean)}
  {@const session = rowSession(row)}
  {@const activity = activityOf(row)}
  {@const navIdx = navIndex(sidebarRowNavKey(row))}
  {@const hint = jumpHintFor(sidebarRowTargetId(row))}
  {@const selected = isSelected(navIdx)}
  {@const active = isActive(row)}
  {@const receded = isQuiet(activity) && !active && !selected}
  {#if row.kind === "session" && rename.sessionId === row.session.id}
    {@const id = row.session.id}
    <SessionRenameInput bind:value={() => rename.value, rename.onInput} onCommit={() => rename.onCommit(id)} onCancel={rename.onCancel} />
  {:else}
    {@const labelId = `sidebar-card-${sidebarRowNavKey(row)}`}
    <!-- The row button lies behind its content so a meta slot can be a button of its own; content clicks bubble here, the slot's do not. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="group relative" {@attach liftWhileDragged(rowId(row))} onclick={() => onSelectRow(row)} onpointerdown={(e) => onRowPress(e, row)} oncontextmenu={(e) => onRowContextMenu(e, row)}>
      <button
        data-nav-index={navIdx}
        data-snooze-anchor={row.kind === "task" ? snoozeAnchor(row.project.id, row.task.key) : undefined}
        class="absolute inset-0 rounded-md transition-colors {rowStateClass(active, selected, selected || isPreviewing(row))}"
        aria-labelledby={labelId}
      ></button>
      <div
        id={labelId}
        class="relative flex w-full cursor-default flex-col gap-1 px-2.5 py-2 transition-opacity {receded ? 'opacity-75 group-hover:opacity-100 group-has-focus-visible:opacity-100' : ''}"
        title={rowTitle(row, activity)}
      >
          {#if showProjects && !inFolder}
            <span class="flex h-5 items-center gap-1.5">
              <span class="min-w-0 flex-1 truncate text-xs text-t2" title={row.project.path}>{row.project.name}</span>
              {@render trailing(row, session, activity, hint)}
            </span>
            <span class="flex min-w-0 items-center gap-1.5">{@render titleParts(row, receded)}</span>
          {:else}
            <span class="flex h-5 items-center gap-1.5">
              {@render titleParts(row, receded)}
              {@render trailing(row, session, activity, hint)}
            </span>
          {/if}
          {#if session}
            {@const [head, tail] = splitBranch(session.branch)}
            <span class="flex h-4 min-w-0 items-center gap-1.5 text-xs text-t2">
              <GitBranch class="size-3.5 shrink-0" />
              <span class="flex min-w-0" title={session.branch}><span class="truncate">{head}</span><span class="shrink-0">{tail}</span></span>
              {#each plugins.meta as item (`${session.id}:${item.plugin.id}:${item.contribution.id}`)}
                <span class="flex shrink-0" data-plugin-session-meta={session.id}>
                  <SessionMetaSlot {item} session={plugins.sessionContext(session)} onNavigate={plugins.onNavigate ?? (() => {})} onClose={plugins.onClose ?? (() => {})} onOpenPanel={plugins.onOpenPanel} />
                </span>
              {/each}
              {#if session.provider}<span class="ml-auto shrink-0 font-mono text-[10px] text-t3">{session.provider}</span>{/if}
            </span>
          {/if}
      </div>
      {#if !hint}{@render hoverActions(row, undefined, activity, active, "top-1.5")}{/if}
    </div>
  {/if}
{/snippet}

{#snippet slim(row: SidebarRow, shelf: ShelfLane)}
  {@const session = rowSession(row)}
  {@const activity = activityOf(row)}
  {@const navIdx = navIndex(sidebarRowNavKey(row))}
  {@const hint = jumpHintFor(sidebarRowTargetId(row))}
  {@const selected = isSelected(navIdx)}
  {@const active = isActive(row)}
  {@const receded = (shelf === "done" || isQuiet(activity)) && !active && !selected}
  <div class="group relative" {@attach liftWhileDragged(rowId(row))}>
    <button
      data-nav-index={navIdx}
      data-snooze-anchor={row.kind === "task" ? snoozeAnchor(row.project.id, row.task.key) : undefined}
      class="flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left transition-colors {rowStateClass(active, selected, selected || isPreviewing(row))}"
      title={rowTitle(row, activity)}
      onclick={() => onSelectRow(row)}
      onpointerdown={(e) => onRowPress(e, row)}
      oncontextmenu={(e) => onRowContextMenu(e, row)}
    >
      <span class="flex min-w-0 flex-1 items-center gap-2 transition-opacity {receded ? 'opacity-75 group-hover:opacity-100 group-has-focus-visible:opacity-100' : ''}">
        {#if showProjects}<span class="max-w-[40%] shrink-0 truncate text-xs text-t3" title={row.project.path}>{row.project.name}</span>{/if}
        {#if row.kind === "task" && showTaskKeys}<span class="shrink-0 font-mono text-[10px] text-t3">{row.task.key}</span>{/if}
        <span class="min-w-0 truncate text-[13px] {shelf === 'done' ? 'text-t3' : 'text-t1'}">{row.kind === "task" ? row.task.title : row.session.name || row.session.branch}</span>
      </span>
      {#if session || activity || hint}<span class="flex h-6 shrink-0 items-center justify-end gap-1.5">{@render trailing(row, session, activity, hint)}</span>{/if}
    </button>
    {#if !hint}{@render hoverActions(row, shelf, activity, active, "top-1")}{/if}
  </div>
{/snippet}

{#snippet folder(section: SidebarFolderSection)}
  {@const project = section.project}
  {@const navIdx = navIndex(navKey.project(section.key))}
  {@const collapsed = isCollapsed(section.key, section.defaultCollapsed)}
  {@const urgency = collapsed ? folderUrgency(section.rows, activityOf) : null}
  {@const drop = laneHover("in_review", project.id)}
  <div class="mt-2 first:mt-0 {laneClass(drop)}" data-task-lane="in_review" data-task-lane-project={project.id}>
    <div class="relative">
      <button
        data-nav-index={navIdx}
        class="flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left hover:bg-sidebar-row-hover {isSelected(navIdx) ? 'ring-2 ring-inset ring-accent' : ''}"
        title={project.path}
        aria-expanded={!collapsed}
        onclick={() => onToggle(section.key, section.defaultCollapsed)}
        oncontextmenu={(e) => onProjectContextMenu(e, project)}
      >
        {#if project.path_missing}<span class="size-4 shrink-0"></span>{/if}
        <span class="min-w-0 truncate text-[13px] font-medium text-t1">{project.name}</span>
        {#if drop}<LaneDropBadge verdict={drop} />{/if}
        {#if urgency}<span class="size-1.5 shrink-0 rounded-full {URGENCY_DOTS[urgency].class}" role="img" aria-label={URGENCY_DOTS[urgency].label} title={URGENCY_DOTS[urgency].label} data-folder-urgency={urgency}></span>{/if}
        {#if projectAutoMode[project.id]}<Zap class="size-2.5 shrink-0 text-status-running" />{/if}
        <span class="ml-auto text-xs tabular-nums text-t3">{section.rows.length}</span>
        {@render chevron(collapsed)}
      </button>
      {#if project.path_missing}
        <button
          type="button"
          class="absolute top-1.5 left-2 flex size-5 items-center justify-center rounded text-status-review hover:bg-sidebar-row-hover"
          title={`Folder not found: ${project.path}`}
          aria-label={`Locate folder for ${project.name}. Folder not found: ${project.path}`}
          data-testid="project-folder-missing"
          onclick={() => locateProjectFolder(project)}
        ><TriangleAlert class="size-3" /></button>
      {/if}
    </div>
    {#if !collapsed && section.rows.length > 0}
      <ul class="flex flex-col gap-0.5">
        {#each section.rows as row (sidebarRowNavKey(row))}
          {@const key = sidebarRowNavKey(row)}
          <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key }} out:send={{ key }} class={rowItemClass(row)}>{@render card(row, true)}</li>
        {/each}
      </ul>
    {/if}
  </div>
{/snippet}

{#snippet shelfSection(section: SidebarShelfSection)}
  {@const navIdx = navIndex(navKey.group(section.key))}
  {@const collapsed = isCollapsed(section.key, section.defaultCollapsed)}
  {@const page = shelfPage(section, shownPages(section.key), collapsed, isActive)}
  {@const moreIdx = navIndex(navKey.more(section.key))}
  {@const drop = laneHover(section.lane, null)}
  <section class={laneClass(drop)} data-task-lane={section.lane}>
    <button
      data-nav-index={navIdx}
      class="flex h-8 w-full items-center gap-2 rounded-md px-2 text-xs font-medium text-t2 {isSelected(navIdx) ? 'ring-2 ring-inset ring-accent' : ''}"
      aria-expanded={!collapsed}
      onclick={() => onToggle(section.key, section.defaultCollapsed)}
    >
      <span class="shrink-0">{LANES[section.lane].label}</span>
      {#if drop}<LaneDropBadge verdict={drop} />{/if}
      <span class="h-px flex-1 bg-border"></span>
      <span class="font-normal tabular-nums text-t3">{section.rows.length}</span>
      {@render chevron(collapsed)}
    </button>
    {#if page.rows.length > 0 || page.more > 0}
      <ul class="flex flex-col gap-0.5">
        {#each page.rows as row (sidebarRowNavKey(row))}
          {@const key = sidebarRowNavKey(row)}
          <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key }} out:send={{ key }} class={rowItemClass(row)}>
            {#if SHELVES[section.lane].variant === "card"}{@render card(row, false)}{:else}{@render slim(row, section.lane)}{/if}
          </li>
        {/each}
        {#if page.more > 0}
          <li>
            <button
              data-nav-index={moreIdx}
              class="flex h-8 w-full items-center rounded-md px-2.5 text-left text-xs text-t3 hover:bg-sidebar-row-hover hover:text-t1 {isSelected(moreIdx) ? 'ring-2 ring-inset ring-accent' : ''}"
              onclick={() => onShowMore(section.key)}
            >Show {page.more} more</button>
          </li>
        {/if}
      </ul>
    {/if}
  </section>
{/snippet}

<!-- One provider for every meta pill, so moving between pills skips the tooltip delay. -->
<Tooltip.Provider delayDuration={300}>
<div class="flex flex-1 flex-col">
  <!-- The top rows and the space under them take a dragged task for review; a folder takes its own project's. -->
  <div class="relative flex flex-1 flex-col {laneClass(topDrop)}" data-task-lane="in_review">
    {#if topDrop}<div class="pointer-events-none absolute top-1 right-1 z-10 flex"><LaneDropBadge verdict={topDrop} /></div>{/if}
    {#each top as section (section.key)}
      {#if section.kind === "folder"}
        {@render folder(section)}
      {:else if section.kind === "attention"}
        <ul class="flex flex-col gap-0.5">
          {#each section.rows as row (sidebarRowNavKey(row))}
            {@const key = sidebarRowNavKey(row)}
            <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key }} out:send={{ key }} class={rowItemClass(row)}>{@render card(row, false)}</li>
          {/each}
        </ul>
      {/if}
    {/each}
  </div>
  {#if shelvesShown.length > 0}
    <!-- Shelves sit at the bottom of the list, below everything that needs attention. -->
    <div class="mt-auto flex flex-col gap-1 pt-3">
      {#each shelvesShown as section (section.key)}{@render shelfSection(section)}{/each}
    </div>
  {/if}
  {#if sections.length === 0}
    <p class="px-3 py-1 text-xs text-t3 italic">No sessions or tasks</p>
  {/if}
</div>
</Tooltip.Provider>
