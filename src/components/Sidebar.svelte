<script lang="ts">
  import type { FocusZone } from "../lib/focus.svelte";
  import { MOD_LABEL } from "../lib/keyboard";
  import { ContextMenu, ResizeHandle } from "./ui";
  import { getLayoutWidth, setLayoutWidth } from "../lib/layout-state";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { GitFork, Plus, LoaderCircle, Lightbulb, Settings, GitPullRequest, GitMerge } from "@lucide/svelte";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Session {
    id: string;
    project_id: string;
    name: string;
    tmux_name: string | null;
    branch: string;
    status: string;
    created_at: string;
    worktree_path: string | null;
    backend: string;
    tab_count: number;
    base_branch: string | null;
    task_key: string | null;
    pr_url: string | null;
    pr_state: string | null;
  }

  interface Props {
    projects: Project[];
    sessions: Session[];
    activeSessionId: string | null;
    zone: FocusZone;
    agentStates: Record<string, string>;
    renamingSessionId: string | null;
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
  }

  let { projects, sessions, activeSessionId, zone, agentStates, renamingSessionId, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onRestartSession, onOpenPreferences, onRenameSession, onStartRename, onArchiveProject, onDeleteProject }: Props = $props();

  let sidebarWidth = $state(getLayoutWidth("sidebar", 224));

  let renameValue = $state("");

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
    if (trimmed) {
      onRenameSession(id, trimmed);
    }
    onStartRename(""); // clear
  }

  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);
  let projectContextMenu = $state<{ x: number; y: number; project: Project } | null>(null);

  function onContextMenu(e: MouseEvent, session: Session) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, session };
  }

  function onProjectContextMenu(e: MouseEvent, project: Project) {
    e.preventDefault();
    projectContextMenu = { x: e.clientX, y: e.clientY, project };
  }

  let selectedIndex = $state(0);

  const grouped = $derived(
    projects.map((p) => ({
      project: p,
      sessions: sessions.filter((s) => s.project_id === p.id),
    }))
  );

  const flatSessionIds = $derived(sessions.map((s) => s.id));

  $effect(() => {
    if (selectedIndex >= flatSessionIds.length) {
      selectedIndex = Math.max(0, flatSessionIds.length - 1);
    }
  });

  function handleKeydown(e: KeyboardEvent) {
    if (zone !== "sidebar" || flatSessionIds.length === 0) return;
    const el = document.activeElement;
    if (el && (el.tagName === "INPUT" || el.tagName === "SELECT" || el.closest("[role='combobox']"))) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, flatSessionIds.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      onSelectSession(flatSessionIds[selectedIndex]);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="relative shrink-0 flex flex-col border-r border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-950 {zone === 'sidebar' ? 'ring-1 ring-inset ring-primary-500/30' : ''}" style:width="{sidebarWidth}px">
  <ResizeHandle side="right" bind:width={sidebarWidth} min={160} max={Infinity} defaultWidth={224} onResizeEnd={(w) => setLayoutWidth("sidebar", w)} />
  <!-- Header -->
  <div class="flex items-center justify-between px-4 py-3 border-b border-surface-200 dark:border-surface-800">
    <span class="text-xs font-semibold text-surface-700 dark:text-surface-300 uppercase tracking-wider">Sessions</span>
    <button
      onclick={onAddProject}
      title="Add project ({MOD_LABEL}N)"
      class="size-6 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors"
    >
      <Plus class="size-4" />
    </button>
  </div>

  <!-- Session list -->
  <nav class="flex-1 overflow-y-auto px-2 py-2 space-y-4">
    {#if projects.length === 0}
      <div class="mt-12 text-center px-4 space-y-3">
        <p class="text-xs text-surface-600 dark:text-surface-400">No projects yet</p>
        <button
          onclick={onAddProject}
          class="text-xs text-primary-600 dark:text-primary-400 hover:underline"
        >Add a project →</button>
      </div>
    {:else}
      {#each grouped as { project, sessions: projectSessions } (project.id)}
        <div>
          <h3 class="px-2 mb-1 text-[11px] font-semibold text-surface-600 dark:text-surface-400 uppercase tracking-wider truncate" title={project.path} oncontextmenu={(e) => onProjectContextMenu(e, project)}>
            {project.name}
          </h3>
          <ul class="space-y-0.5">
            {#each projectSessions as session (session.id)}
              {@const globalIndex = flatSessionIds.indexOf(session.id)}
              {@const isActive = session.id === activeSessionId}
              {@const isSelected = zone === 'sidebar' && globalIndex === selectedIndex}
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
                    {isActive
                      ? 'bg-primary-500/15 text-primary-700 dark:text-surface-50 font-medium'
                      : 'text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800'}
                    {isSelected ? 'ring-1 ring-primary-500/50' : ''}
                    {session.status === 'exited' ? 'opacity-60' : ''}"
                  onclick={() => onSelectSession(session.id)}
                  oncontextmenu={(e) => onContextMenu(e, session)}
                >
                  <span class="w-3 shrink-0 text-center text-surface-600 dark:text-surface-400" title={session.worktree_path ? "Worktree" : ""}>{#if session.worktree_path}<GitFork class="size-3" />{/if}</span>
                  {#if isActive}<span class="size-1.5 rounded-full bg-primary-500 shrink-0"></span>{/if}
                  {#if session.task_key}<span class="shrink-0 text-[10px] font-medium text-primary-600 dark:text-primary-400">{session.task_key}</span>{/if}
                  <span class="truncate">{session.name || session.branch}</span>
                  {#if session.pr_url}
                    <button
                      class="ml-auto shrink-0 size-3.5 {session.pr_state === 'merged' ? 'text-purple-600 dark:text-purple-400' : session.pr_state === 'draft' ? 'text-surface-500 dark:text-surface-400' : 'text-green-600 dark:text-green-400'}"
                      title="Open PR ({session.pr_state})"
                      tabindex="-1"
                      onmousedown={(e: MouseEvent) => e.preventDefault()}
                      onclick={(e: MouseEvent) => { e.stopPropagation(); openUrl(session.pr_url!); }}
                    >
                      {#if session.pr_state === "merged"}
                        <GitMerge class="size-3.5" />
                      {:else}
                        <GitPullRequest class="size-3.5" />
                      {/if}
                    </button>
                  {/if}
                  {#if session.status === 'exited'}
                    <span class="ml-auto shrink-0 text-[10px] font-medium text-surface-500 dark:text-surface-400 bg-surface-200 dark:bg-surface-800 rounded px-1" title="{session.backend} session exited">exited</span>
                  {:else if agentStates[session.id] === 'Busy'}
                    <span class="ml-auto shrink-0 size-3.5 animate-spin text-surface-500" title="Agent working">
                      <LoaderCircle class="size-3.5" />
                    </span>
                  {:else if agentStates[session.id] === 'Idle'}
                    <span class="ml-auto shrink-0 size-3.5 animate-pulse text-amber-500" title="Needs attention">
                      <Lightbulb class="size-3.5" />
                    </span>
                  {/if}
                </button>
                {/if}
              </li>
            {/each}
            {#if projectSessions.length === 0}
              <li class="px-2 py-1 text-xs text-surface-600 dark:text-surface-400 italic">No sessions</li>
            {/if}
          </ul>
        </div>
      {/each}
    {/if}
  </nav>

  <!-- Settings -->
  <div class="px-3 py-2 border-t border-surface-200 dark:border-surface-800">
    <button
      onclick={onOpenPreferences}
      title="Preferences ({MOD_LABEL},)"
      class="size-7 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors"
    >
      <Settings class="size-4" />
    </button>
  </div>
</aside>

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

{#if projectContextMenu}
  <ContextMenu
    x={projectContextMenu.x}
    y={projectContextMenu.y}
    onClose={() => (projectContextMenu = null)}
    items={[
      { label: "Archive project", onSelect: () => onArchiveProject(projectContextMenu!.project) },
      { label: "Delete project", danger: true, onSelect: () => onDeleteProject(projectContextMenu!.project) },
    ]}
  />
{/if}
