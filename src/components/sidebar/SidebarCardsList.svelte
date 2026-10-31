<script lang="ts">
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { crossfade } from "svelte/transition";
  import { Check, ChevronDown, CircleAlert, CircleCheck, CircleDashed, GitBranch, GitCompare, Play, RotateCcw, TriangleAlert, Undo2, Zap, type LucideIcon } from "@lucide/svelte";
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import { getActiveZone } from "../../lib/focus.svelte";
  import { getSelectedIndex } from "../../lib/sidebar-nav.svelte";
  import { locateProjectFolder } from "../../lib/locate-project";
  import * as orchestrator from "../../lib/session-orchestrator.svelte";
  import { SHELVES, TASK_STATUS_LABELS, navKey, shelfPage, sidebarRowNavKey, type IsCollapsed, type ShelfStatus, type ShownPages, type SidebarFolderSection, type SidebarRow, type SidebarSection, type SidebarShelfSection } from "../../lib/sidebar-model";
  import { folderUrgency, rowActions, rowSession, sessionActivity, splitBranch, type FolderUrgency, type RowAction, type SessionActivity } from "../../lib/sidebar-cards";
  import type { PluginSessionContext } from "../../lib/plugin-sdk";
  import type { PluginInventory, PluginUiContribution, Project, Session } from "../../lib/types";

  type PluginSlot = { plugin: PluginInventory; contribution: PluginUiContribution };

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
    renamingSessionId: string | null;
    renameValue: string;
    onCommitRename: (sessionId: string) => void;
    onCancelRename: () => void;
    projectAutoMode: Record<string, boolean>;
    onSelectRow: (row: SidebarRow) => void;
    onRowAction: (action: RowAction, row: SidebarRow) => void;
    onRowContextMenu: (e: MouseEvent, row: SidebarRow) => void;
    onProjectContextMenu: (e: MouseEvent, project: Project) => void;
    sessionIndicatorContributions: PluginSlot[];
    sessionMetaContributions: PluginSlot[];
    pluginSessionContext: (session: Session) => PluginSessionContext;
    onPluginNavigate: (pluginId: string, contributionId: string) => void;
    onPluginClose: () => void;
  }

  let {
    sections, showTaskKeys, showProjects, navIndex, isCollapsed, onToggle, shownPages, onShowMore,
    isActive, isPreviewing, isFading, renamingSessionId, renameValue = $bindable(), onCommitRename, onCancelRename,
    projectAutoMode, onSelectRow, onRowAction, onRowContextMenu, onProjectContextMenu,
    sessionIndicatorContributions, sessionMetaContributions, pluginSessionContext, onPluginNavigate, onPluginClose,
  }: Props = $props();

  const ACTIVITY_SLOTS: Record<SessionActivity, { icon: LucideIcon; label: string; class: string }> = {
    working: { icon: CircleDashed, label: "Working", class: "text-status-running" },
    ready: { icon: CircleCheck, label: "Ready", class: "text-status-review" },
    exited: { icon: CircleAlert, label: "Exited", class: "text-status-exited" },
  };

  const ACTIONS: Record<RowAction, { icon: LucideIcon; label: string; text?: string }> = {
    review: { icon: GitCompare, label: "Review diff" },
    restart: { icon: RotateCcw, label: "Restart session" },
    done: { icon: Check, label: "Mark done", text: "Done" },
    archive: { icon: Check, label: "Archive session", text: "Done" },
    start: { icon: Play, label: "Start session", text: "Start" },
    reopen: { icon: Undo2, label: "Reopen", text: "Reopen" },
  };

  const URGENCY_DOTS: Record<FolderUrgency, string> = {
    exited: "bg-status-exited",
    review: "bg-status-review",
    running: "bg-status-running",
  };

  const top = $derived(sections.filter((s) => s.kind === "attention" || s.kind === "folder"));
  const shelves = $derived(sections.filter((s): s is SidebarShelfSection => s.kind === "shelf"));
  const agentStates = $derived(orchestrator.getAgentStates());
  const reviewReady = $derived(orchestrator.getReviewReady());

  const activityOf = (row: SidebarRow) => sessionActivity(rowSession(row), agentStates, reviewReady);
  const isSelected = (navIdx: number) => getActiveZone() === "sidebar" && navIdx === getSelectedIndex();

  const motionMs = () => (window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 150);
  // A row moving between lists glides from its old place; a row without a counterpart fades and slides in or out.
  const [send, receive] = crossfade({
    duration: motionMs,
    easing: cubicOut,
    fallback: () => ({ duration: motionMs(), easing: cubicOut, css: (t) => `opacity: ${t}; transform: translateY(${(t - 1) * 8}px)` }),
  });

  function rowStateClass(active: boolean, selected: boolean, ringed: boolean): string {
    const background = active ? "bg-sidebar-row-active" : selected ? "bg-sidebar-row-selected" : "hover:bg-sidebar-row-hover";
    const ring = ringed ? "ring-2 ring-inset ring-accent" : active ? "ring-1 ring-inset ring-border-s" : "";
    return `${background} ${ring}`;
  }

  function autofocus(node: HTMLInputElement) { requestAnimationFrame(() => node.focus()); }
</script>

{#snippet chevron(collapsed: boolean)}
  <ChevronDown class="size-3 shrink-0 text-t3 transition-transform {collapsed ? '-rotate-90' : ''}" />
{/snippet}

{#snippet trailing(session: Session | undefined, activity: SessionActivity | null)}
  {#if session}
    {#each sessionIndicatorContributions as item (`${session.id}:${item.plugin.id}:${item.contribution.id}`)}
      <span class="flex size-4 shrink-0 has-[[data-plugin-indicator-visible=false]]:hidden" data-plugin-session-indicator={session.id}>
        <PluginContributionHost plugin={item.plugin} contribution={item.contribution} session={pluginSessionContext(session)} onNavigate={onPluginNavigate} onClose={onPluginClose} />
      </span>
    {/each}
  {/if}
  {#if activity}
    {@const slot = ACTIVITY_SLOTS[activity]}
    <span class="flex shrink-0 items-center gap-1 text-xs tabular-nums transition-opacity group-focus-within:opacity-0 group-hover:opacity-0 {slot.class}">
      <slot.icon class="size-3.5" />{slot.label}
    </span>
  {/if}
{/snippet}

{#snippet hoverActions(row: SidebarRow, shelf: ShelfStatus | undefined, active: boolean, position: string)}
  <div class="pointer-events-none absolute right-2.5 flex h-6 items-center rounded-md bg-sidebar opacity-0 transition-opacity group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100 {position}">
    <!-- Layered over an opaque base so the actions cover the row content with the row's own background. -->
    <div class="flex h-full items-center gap-0.5 rounded-md {active ? 'bg-sidebar-row-active' : 'bg-sidebar-row-hover'}">
      {#each rowActions(row, shelf) as action (action)}
        {@const spec = ACTIONS[action]}
        <button
          type="button"
          class="flex items-center rounded-md text-t2 hover:bg-sidebar-row-hover hover:text-t1 {spec.text ? 'h-6 gap-1 px-1.5 text-xs' : 'size-6 justify-center'}"
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
  {@const selected = isSelected(navIdx)}
  {@const active = isActive(row)}
  {@const receded = (activity === null || activity === "exited") && !active && !selected}
  {#if row.kind === "session" && renamingSessionId === row.session.id}
    {@const id = row.session.id}
    <input
      use:autofocus
      class="w-full px-2 py-1.5 rounded-md text-sm bg-panel border border-accent outline-none text-t1"
      bind:value={renameValue}
      onkeydown={(e) => { if (e.key === "Enter") onCommitRename(id); if (e.key === "Escape") onCancelRename(); }}
      onblur={() => onCommitRename(id)}
    />
  {:else}
    <div class="group relative">
      <button
        data-nav-index={navIdx}
        class="flex w-full flex-col gap-1 rounded-md px-2.5 py-2 text-left transition-colors {rowStateClass(active, selected, selected || isPreviewing(row))}"
        title={row.kind === "task" && !showTaskKeys ? row.task.key : undefined}
        onclick={() => onSelectRow(row)}
        oncontextmenu={(e) => onRowContextMenu(e, row)}
      >
        <span class="flex w-full flex-col gap-1 transition-opacity {receded ? 'opacity-75 group-hover:opacity-100 group-has-focus-visible:opacity-100' : ''}">
          {#if showProjects && !inFolder}
            <span class="flex h-5 items-center gap-1.5">
              <span class="min-w-0 flex-1 truncate text-xs text-t2" title={row.project.path}>{row.project.name}</span>
              {@render trailing(session, activity)}
            </span>
            <span class="flex min-w-0 items-center gap-1.5">{@render titleParts(row, receded)}</span>
          {:else}
            <span class="flex h-5 items-center gap-1.5">
              {@render titleParts(row, receded)}
              {@render trailing(session, activity)}
            </span>
          {/if}
          {#if session}
            {@const [head, tail] = splitBranch(session.branch)}
            <span class="flex h-4 min-w-0 items-center gap-1.5 text-xs text-t2">
              <GitBranch class="size-3.5 shrink-0" />
              <span class="flex min-w-0" title={session.branch}><span class="truncate">{head}</span><span class="shrink-0">{tail}</span></span>
              {#each sessionMetaContributions as item (`${session.id}:${item.plugin.id}:${item.contribution.id}`)}
                <span class="flex shrink-0" data-plugin-session-meta={session.id}>
                  <PluginContributionHost plugin={item.plugin} contribution={item.contribution} session={pluginSessionContext(session)} onNavigate={onPluginNavigate} onClose={onPluginClose} />
                </span>
              {/each}
              {#if session.provider}<span class="ml-auto shrink-0 font-mono text-[10px] text-t3">{session.provider}</span>{/if}
            </span>
          {/if}
        </span>
      </button>
      {@render hoverActions(row, undefined, active, "top-1.5")}
    </div>
  {/if}
{/snippet}

{#snippet slim(row: SidebarRow, shelf: ShelfStatus)}
  {@const session = rowSession(row)}
  {@const activity = activityOf(row)}
  {@const navIdx = navIndex(sidebarRowNavKey(row))}
  {@const selected = isSelected(navIdx)}
  {@const active = isActive(row)}
  {@const receded = (shelf === "done" || activity === null || activity === "exited") && !active && !selected}
  <div class="group relative">
    <button
      data-nav-index={navIdx}
      class="flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left transition-colors {rowStateClass(active, selected, selected || isPreviewing(row))}"
      title={row.kind === "task" && !showTaskKeys ? row.task.key : undefined}
      onclick={() => onSelectRow(row)}
      oncontextmenu={(e) => onRowContextMenu(e, row)}
    >
      <span class="flex min-w-0 flex-1 items-center gap-2 transition-opacity {receded ? 'opacity-75 group-hover:opacity-100 group-has-focus-visible:opacity-100' : ''}">
        {#if showProjects}<span class="max-w-[40%] shrink-0 truncate text-xs text-t3" title={row.project.path}>{row.project.name}</span>{/if}
        {#if row.kind === "task" && showTaskKeys}<span class="shrink-0 font-mono text-[10px] text-t3">{row.task.key}</span>{/if}
        <span class="min-w-0 truncate text-[13px] {shelf === 'done' ? 'text-t3' : 'text-t1'}">{row.kind === "task" ? row.task.title : row.session.name || row.session.branch}</span>
      </span>
      {#if session}<span class="flex h-6 shrink-0 items-center justify-end gap-1.5">{@render trailing(session, activity)}</span>{/if}
    </button>
    {@render hoverActions(row, shelf, active, "top-1")}
  </div>
{/snippet}

{#snippet folder(section: SidebarFolderSection)}
  {@const project = section.project}
  {@const navIdx = navIndex(navKey.project(section.key))}
  {@const collapsed = isCollapsed(section.key, section.defaultCollapsed)}
  {@const urgency = collapsed ? folderUrgency(section.rows, activityOf) : null}
  <div class="mt-2 first:mt-0">
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
        {#if urgency}<span class="size-1.5 shrink-0 rounded-full {URGENCY_DOTS[urgency]}" data-folder-urgency={urgency}></span>{/if}
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
          <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key: sidebarRowNavKey(row) }} out:send={{ key: sidebarRowNavKey(row) }} class="transition-opacity duration-200 {isFading(row) ? 'opacity-0' : 'opacity-100'}">{@render card(row, true)}</li>
        {/each}
      </ul>
    {/if}
  </div>
{/snippet}

{#snippet shelfSection(section: SidebarShelfSection)}
  {@const navIdx = navIndex(navKey.group(section.key))}
  {@const collapsed = isCollapsed(section.key, section.defaultCollapsed)}
  {@const page = shelfPage(section, shownPages(section.key))}
  {@const moreIdx = navIndex(navKey.more(section.key))}
  <section>
    <button
      data-nav-index={navIdx}
      class="flex h-8 w-full items-center gap-2 rounded-md px-2 text-xs font-medium text-t2 {isSelected(navIdx) ? 'ring-2 ring-inset ring-accent' : ''}"
      aria-expanded={!collapsed}
      onclick={() => onToggle(section.key, section.defaultCollapsed)}
    >
      <span class="shrink-0">{TASK_STATUS_LABELS[section.status]}</span>
      <span class="h-px flex-1 bg-border"></span>
      <span class="font-normal tabular-nums text-t3">{section.rows.length}</span>
      {@render chevron(collapsed)}
    </button>
    {#if !collapsed}
      <ul class="flex flex-col gap-0.5">
        {#each page.rows as row (sidebarRowNavKey(row))}
          <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key: sidebarRowNavKey(row) }} out:send={{ key: sidebarRowNavKey(row) }} class="transition-opacity duration-200 {isFading(row) ? 'opacity-0' : 'opacity-100'}">
            {#if SHELVES[section.status].variant === "card"}{@render card(row, false)}{:else}{@render slim(row, section.status)}{/if}
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

<div class="flex flex-1 flex-col">
  {#each top as section (section.key)}
    {#if section.kind === "folder"}
      {@render folder(section)}
    {:else if section.kind === "attention"}
      <ul class="flex flex-col gap-0.5">
        {#each section.rows as row (sidebarRowNavKey(row))}
          <li animate:flip={{ duration: motionMs, easing: cubicOut }} in:receive={{ key: sidebarRowNavKey(row) }} out:send={{ key: sidebarRowNavKey(row) }} class="transition-opacity duration-200 {isFading(row) ? 'opacity-0' : 'opacity-100'}">{@render card(row, false)}</li>
        {/each}
      </ul>
    {/if}
  {/each}
  {#if shelves.length > 0}
    <!-- Shelves sit at the bottom of the list, below everything that needs attention. -->
    <div class="mt-auto flex flex-col gap-1 pt-3">
      {#each shelves as section (section.key)}{@render shelfSection(section)}{/each}
    </div>
  {/if}
  {#if sections.length === 0}
    <p class="px-3 py-1 text-xs text-t3 italic">No sessions or tasks</p>
  {/if}
</div>
