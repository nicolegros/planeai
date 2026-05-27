<script lang="ts">
  import type { FocusZone } from "../lib/focus.svelte";
  import { MOD_LABEL } from "../lib/keyboard";
  import { ContextMenu } from "./ui";

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
  }

  let { projects, sessions, activeSessionId, zone, agentStates, renamingSessionId, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onRestartSession, onOpenPreferences, onRenameSession, onStartRename }: Props = $props();

  let renameValue = $state("");

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

  function onContextMenu(e: MouseEvent, session: Session) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, session };
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

<aside class="w-56 flex flex-col border-r border-surface-200 dark:border-surface-800 bg-surface-100 dark:bg-surface-950 {zone === 'sidebar' ? 'ring-1 ring-inset ring-primary-500/30' : ''}">
  <!-- Header -->
  <div class="flex items-center justify-between px-4 py-3 border-b border-surface-200 dark:border-surface-800">
    <span class="text-xs font-semibold text-surface-700 dark:text-surface-300 uppercase tracking-wider">Sessions</span>
    <button
      onclick={onAddProject}
      title="Add project ({MOD_LABEL}N)"
      class="size-6 flex items-center justify-center rounded text-surface-600 hover:text-surface-700 hover:bg-surface-200 dark:text-surface-300 dark:hover:text-surface-200 dark:hover:bg-surface-800 transition-colors"
    >
      <svg class="size-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path d="M12 5v14m-7-7h14"/></svg>
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
          <h3 class="px-2 mb-1 text-[11px] font-semibold text-surface-600 dark:text-surface-400 uppercase tracking-wider truncate" title={project.path}>
            {project.name}
          </h3>
          <ul class="space-y-0.5">
            {#each projectSessions as session (session.id)}
              {@const globalIndex = flatSessionIds.indexOf(session.id)}
              {@const isActive = session.id === activeSessionId}
              {@const isSelected = zone === 'sidebar' && globalIndex === selectedIndex}
              <li>
                {#if renamingSessionId === session.id}
                  <!-- svelte-ignore a11y_autofocus -->
                  <input
                    autofocus
                    class="w-full px-2 py-1.5 rounded-md text-sm bg-surface-50 dark:bg-surface-800 border border-primary-500 outline-none"
                    bind:value={renameValue}
                    onkeydown={(e) => { if (e.key === 'Enter') commitRename(session.id); if (e.key === 'Escape') onStartRename(""); }}
                    onblur={() => commitRename(session.id)}
                  />
                {:else}
                <button
                  class="w-full text-left px-2 py-1.5 rounded-md text-sm flex items-center gap-1 transition-colors
                    {isActive
                      ? 'bg-primary-500/15 text-primary-700 dark:text-primary-300 font-medium'
                      : 'text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800'}
                    {isSelected ? 'ring-1 ring-primary-500/50' : ''}
                    {session.status === 'exited' ? 'opacity-60' : ''}"
                  onclick={() => onSelectSession(session.id)}
                  oncontextmenu={(e) => onContextMenu(e, session)}
                >
                  <span class="w-3 shrink-0 text-center text-[10px] text-surface-600 dark:text-surface-400" title={session.worktree_path ? "Worktree" : ""}>{session.worktree_path ? '⎇' : ''}</span>
                  {#if isActive}<span class="size-1.5 rounded-full bg-primary-500 shrink-0"></span>{/if}
                  <span class="truncate">{session.name || session.branch}</span>
                  {#if session.status === 'exited'}
                    <span class="ml-auto shrink-0 text-[10px] font-medium text-surface-500 dark:text-surface-400 bg-surface-200 dark:bg-surface-800 rounded px-1" title="{session.backend} session exited">exited</span>
                  {:else if agentStates[session.id] === 'Busy'}
                    <span class="ml-auto shrink-0 size-3.5 animate-spin text-surface-500" title="Agent working">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48-8.48l2.83-2.83M2 12h4m12 0h4m-3.93 7.07l-2.83-2.83M7.76 7.76L4.93 4.93"/></svg>
                    </span>
                  {:else if agentStates[session.id] === 'Idle'}
                    <span class="ml-auto shrink-0 size-3.5 animate-pulse text-amber-500" title="Needs attention">
                      <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a7 7 0 0 0-7 7c0 3.53 2.13 6.5 4 8.5V19a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1v-1.5c1.87-2 4-4.97 4-8.5a7 7 0 0 0-7-7zm-1 19h2a1 1 0 0 1 0 2h-2a1 1 0 0 1 0-2z"/></svg>
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
      <svg class="size-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>
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
