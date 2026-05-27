<script lang="ts">
  import { Command, Dialog } from "bits-ui";
  import { invoke } from "@tauri-apps/api/core";

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

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Props {
    open: boolean;
    sessions: Session[];
    projects: Project[];
    activeSessionId: string | null;
    onOpenChange: (open: boolean) => void;
    onSelectSession: (id: string) => void;
    onArchiveSession: () => void;
    onDeleteSession: () => void;
    onNewSession: () => void;
    onRenameSession: () => void;
    onRestoreSession: (id: string) => void;
    onDestroyArchivedSession: (id: string, tmuxName: string) => void;
  }

  let { open, sessions, projects, activeSessionId, onOpenChange, onSelectSession, onArchiveSession, onDeleteSession, onNewSession, onRenameSession, onRestoreSession, onDestroyArchivedSession }: Props = $props();

  let archivedSessions = $state<Session[]>([]);
  let showArchived = $state(false);

  async function openArchived() {
    archivedSessions = await invoke<Session[]>("list_archived_sessions");
    showArchived = true;
  }

  function projectName(projectId: string): string {
    return projects.find((p) => p.id === projectId)?.name ?? "unknown";
  }

  function close() {
    showArchived = false;
    onOpenChange(false);
  }

  function getActiveRootPath(): string | null {
    const session = sessions.find((s) => s.id === activeSessionId);
    if (!session) return null;
    if (session.worktree_path) return session.worktree_path;
    const project = projects.find((p) => p.id === session.project_id);
    return project?.path ?? null;
  }

  function copyRootPath() {
    const path = getActiveRootPath();
    if (path) navigator.clipboard.writeText(path).catch(() => {});
    close();
  }
</script>

<Dialog.Root {open} onOpenChange={(v) => { if (!v) close(); else onOpenChange(v); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-50 bg-black/50" />
    <Dialog.Content class="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-200 bg-surface-50 shadow-lg overflow-hidden dark:border-surface-700 dark:bg-surface-900">
      <Dialog.Title class="sr-only">Command Menu</Dialog.Title>
      <Dialog.Description class="sr-only">Search sessions, archive, or create new.</Dialog.Description>
      {#if showArchived}
        <Command.Root class="flex flex-col" loop>
          <Command.Input
            class="h-11 w-full border-b border-surface-200 bg-transparent px-4 text-sm outline-none placeholder:text-surface-400 dark:border-surface-700 dark:placeholder:text-surface-500"
            placeholder="Search archived sessions..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-sm text-surface-700 dark:text-surface-300">
                No archived sessions.
              </Command.Empty>
              <Command.Group>
                <Command.GroupHeading class="px-3 pb-1 pt-3 text-xs text-surface-700 dark:text-surface-300">Archived Sessions</Command.GroupHeading>
                <Command.GroupItems>
                  {#each archivedSessions as session (session.id)}
                    <Command.Item
                      value="restore {session.name || session.branch} {projectName(session.project_id)}"
                      keywords={[session.name, session.branch, projectName(session.project_id)]}
                      class="flex h-9 cursor-pointer items-center justify-between rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800"
                      onSelect={() => { onRestoreSession(session.id); archivedSessions = archivedSessions.filter(s => s.id !== session.id); }}
                    >
                      <span class="truncate">{session.name || session.branch} <span class="text-xs text-surface-500">({projectName(session.project_id)})</span></span>
                      <span class="text-xs text-primary-600 dark:text-primary-400 shrink-0 ml-2">Restore</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
              <Command.Separator class="my-1 h-px bg-surface-100 dark:bg-surface-800" />
              <Command.Group>
                <Command.GroupHeading class="px-3 pb-1 pt-3 text-xs text-surface-700 dark:text-surface-300">Delete Archived</Command.GroupHeading>
                <Command.GroupItems>
                  {#each archivedSessions as session (session.id)}
                    <Command.Item
                      value="delete {session.name || session.branch} {projectName(session.project_id)}"
                      keywords={[session.name, session.branch, "delete", "destroy"]}
                      class="flex h-9 cursor-pointer items-center justify-between rounded-md px-3 text-sm text-error-600 dark:text-error-400 data-selected:bg-surface-100 dark:data-selected:bg-surface-800"
                      onSelect={() => { onDestroyArchivedSession(session.id, session.tmux_name); archivedSessions = archivedSessions.filter(s => s.id !== session.id); }}
                    >
                      <span class="truncate">{session.name || session.branch} <span class="text-xs opacity-70">({projectName(session.project_id)})</span></span>
                      <span class="text-xs shrink-0 ml-2">Delete</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else}
      <Command.Root class="flex flex-col" loop>
        <Command.Input
          class="h-11 w-full border-b border-surface-200 bg-transparent px-4 text-sm outline-none placeholder:text-surface-400 dark:border-surface-700 dark:placeholder:text-surface-500"
          placeholder="Search sessions..."
        />
        <Command.List class="max-h-72 overflow-y-auto p-2">
          <Command.Viewport>
            <Command.Empty class="flex items-center justify-center py-6 text-sm text-surface-700 dark:text-surface-300">
              No results found.
            </Command.Empty>

            <Command.Group>
              <Command.GroupHeading class="px-3 pb-1 pt-3 text-xs text-surface-700 dark:text-surface-300">Sessions</Command.GroupHeading>
              <Command.GroupItems>
                {#each sessions as session (session.id)}
                  <Command.Item
                    value="session {session.name || session.branch} {session.id}"
                    keywords={[session.name, session.branch]}
                    class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800 {session.id === activeSessionId ? 'font-medium' : ''}"
                    onSelect={() => { onSelectSession(session.id); close(); }}
                  >
                    {session.name || session.branch}
                  </Command.Item>
                {/each}
              </Command.GroupItems>
            </Command.Group>

            <Command.Separator class="my-1 h-px bg-surface-100 dark:bg-surface-800" />

            <Command.Group>
              <Command.GroupHeading class="px-3 pb-1 pt-3 text-xs text-surface-700 dark:text-surface-300">Actions</Command.GroupHeading>
              <Command.GroupItems>
                <Command.Item
                  value="copy project root path"
                  keywords={["copy", "path", "root", "directory", "cwd"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800 aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={copyRootPath}
                >
                  Copy project root path
                </Command.Item>
                <Command.Item
                  value="rename current session"
                  keywords={["rename", "name", "edit"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800 aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onRenameSession(); close(); }}
                >
                  Rename session
                </Command.Item>
                <Command.Item
                  value="archive current session"
                  keywords={["archive", "close", "stop"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800 aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onArchiveSession(); close(); }}
                >
                  Archive current session
                </Command.Item>
                <Command.Item
                  value="delete current session"
                  keywords={["delete", "destroy", "remove", "kill"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-error-600 dark:text-error-400 data-selected:bg-surface-100 dark:data-selected:bg-surface-800 aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onDeleteSession(); close(); }}
                >
                  Delete current session
                </Command.Item>
                <Command.Item
                  value="create new session"
                  keywords={["new", "create", "add"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800"
                  onSelect={() => { onNewSession(); close(); }}
                >
                  New session
                </Command.Item>
                <Command.Item
                  value="archived sessions"
                  keywords={["archived", "restore", "old", "hidden"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-md px-3 text-sm text-surface-700 dark:text-surface-300 data-selected:bg-surface-100 dark:data-selected:bg-surface-800"
                  onSelect={openArchived}
                >
                  Archived sessions
                </Command.Item>
              </Command.GroupItems>
            </Command.Group>
          </Command.Viewport>
        </Command.List>
      </Command.Root>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
