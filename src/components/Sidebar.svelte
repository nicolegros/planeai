<script lang="ts">
  import type { FocusZone } from "../lib/focus.svelte";
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
    tmux_name: string;
    branch: string;
    status: string;
    created_at: string;
    worktree_path: string | null;
  }

  interface Props {
    projects: Project[];
    sessions: Session[];
    activeSessionId: string | null;
    zone: FocusZone;
    onAddProject: () => void;
    onSelectSession: (id: string) => void;
    onArchiveSession: (session: Session) => void;
    onDeleteSession: (session: Session) => void;
    onOpenPreferences: () => void;
  }

  let { projects, sessions, activeSessionId, zone, onAddProject, onSelectSession, onArchiveSession, onDeleteSession, onOpenPreferences }: Props = $props();

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
      title="Add project (⌘N)"
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
                <button
                  class="w-full text-left px-2 py-1.5 rounded-md text-sm truncate flex items-center gap-1.5 transition-colors
                    {isActive
                      ? 'bg-primary-500/15 text-primary-700 dark:text-primary-300 font-medium'
                      : 'text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800'}
                    {isSelected ? 'ring-1 ring-primary-500/50' : ''}"
                  onclick={() => onSelectSession(session.id)}
                  oncontextmenu={(e) => onContextMenu(e, session)}
                >
                  {#if session.worktree_path}
                    <span class="text-surface-600 dark:text-surface-400 text-xs shrink-0" title="Worktree">⑂</span>
                  {/if}
                  {#if isActive}
                    <span class="size-1.5 rounded-full bg-primary-500 shrink-0"></span>
                  {/if}
                  <span class="truncate">{session.name || session.branch}</span>
                </button>
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
      title="Preferences (⌘,)"
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
    items={[
      { label: "Archive", onSelect: () => onArchiveSession(contextMenu!.session) },
      { label: "Delete", danger: true, onSelect: () => onDeleteSession(contextMenu!.session) },
    ]}
  />
{/if}
