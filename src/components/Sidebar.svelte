<script lang="ts">
  import type { FocusZone } from "../lib/focus.svelte";

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
  }

  let { projects, sessions, activeSessionId, zone, onAddProject, onSelectSession, onArchiveSession, onDeleteSession }: Props = $props();

  let contextMenu = $state<{ x: number; y: number; session: Session } | null>(null);

  function onContextMenu(e: MouseEvent, session: Session) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, session };
  }

  function closeContextMenu() {
    contextMenu = null;
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

<aside class="w-56 border-r border-gray-200 flex flex-col bg-gray-50 {zone === 'sidebar' ? 'bg-gray-100' : ''}">
  <div class="flex items-center justify-between p-3 pb-1">
    <h2 class="text-xs font-semibold text-gray-500 uppercase tracking-wide">Sessions</h2>
    <button onclick={onAddProject} class="rounded p-1 text-sm hover:bg-gray-200" title="Add project">+</button>
  </div>

  <div class="flex-1 overflow-y-auto p-3 pt-1">
    {#if projects.length === 0}
      <div class="mt-8 text-center space-y-2">
        <p class="text-xs text-gray-500">No projects registered.</p>
        <button onclick={onAddProject} class="rounded border border-gray-300 px-2 py-1 text-xs">Add a project</button>
      </div>
    {:else}
      {#each grouped as { project, sessions: projectSessions } (project.id)}
        <div class="mb-3">
          <p class="text-xs text-gray-500 font-medium mb-1">{project.name}</p>
          {#each projectSessions as session (session.id)}
            {@const globalIndex = flatSessionIds.indexOf(session.id)}
            <button
              class="w-full text-left px-2 py-1 rounded text-sm truncate
                {session.id === activeSessionId ? 'bg-gray-900 text-white' : 'text-gray-600 hover:bg-gray-200'}
                {zone === 'sidebar' && globalIndex === selectedIndex ? 'ring-1 ring-blue-500' : ''}"
              onclick={() => onSelectSession(session.id)}
              oncontextmenu={(e) => onContextMenu(e, session)}
            >
              {#if session.worktree_path}<span class="inline-block mr-1 opacity-60" title="Worktree">⑂</span>{/if}{session.name || session.branch}
            </button>
          {/each}
          {#if projectSessions.length === 0}
            <p class="text-xs text-gray-400 px-2">No sessions</p>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</aside>

{#if contextMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-50" onclick={closeContextMenu} oncontextmenu={(e) => { e.preventDefault(); closeContextMenu(); }}>
    <div
      class="absolute rounded border border-gray-200 bg-white shadow-lg py-1 text-sm w-40"
      style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    >
      <button class="w-full text-left px-3 py-1.5 hover:bg-gray-100" onclick={() => { onArchiveSession(contextMenu!.session); closeContextMenu(); }}>Archive</button>
      <button class="w-full text-left px-3 py-1.5 hover:bg-gray-100 text-red-600" onclick={() => { onDeleteSession(contextMenu!.session); closeContextMenu(); }}>Delete</button>
    </div>
  </div>
{/if}
