<script lang="ts">
  import { Command, Dialog } from "bits-ui";
  import { MOD_LABEL } from "../lib/keyboard";
  import { sessions as sessionsApi, projects as projectsApi, tasks as tasksApi, git } from "../lib/api";
  import type { Session, Project, TaskItem } from "../lib/types";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getSettings, updateSettings } from "../lib/settings.svelte";
  import * as orchestrator from "../lib/session-orchestrator.svelte";
  import * as projectStore from "../lib/project-store.svelte";

  interface Props {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSelectSession: (id: string) => void;
    onArchiveSession: () => void;
    onDeleteSession: () => void;
    onNewSession: () => void;
    onRenameSession: () => void;
    onRestoreSession: (id: string) => void;
    onDestroyArchivedSession: (id: string) => void;
    onResetTerminal: () => void;
    onArchiveProject: (id: string) => void;
    onHideProject: (id: string) => void | Promise<void>;
    onUnhideProject: (id: string) => void | Promise<void>;
    onDeleteProject: (id: string) => void;
    onRestoreProject: (id: string) => void;
    onPickTask: (task: TaskItem) => void;
    onCreateTask: () => void;
    onToggleDiff: () => void;
    onOpenFile?: (filePath: string) => void;
    onOpenLogViewer?: () => void;
    onCreatePr?: () => void;
    onSplitVertical?: () => void;
    onSplitHorizontal?: () => void;
    onCloseSplit?: () => void;
    openFileMode?: boolean;
  }

  let { open, onOpenChange, onSelectSession, onArchiveSession, onDeleteSession, onNewSession, onRenameSession, onRestoreSession, onDestroyArchivedSession, onResetTerminal, onArchiveProject, onHideProject, onUnhideProject, onDeleteProject, onRestoreProject, onPickTask, onCreateTask, onToggleDiff, onOpenFile, onOpenLogViewer, onCreatePr, onSplitVertical, onSplitHorizontal, onCloseSplit, openFileMode = false }: Props = $props();

  // ─── Derived from stores ────────────────────────────────────────────────────
  const sessions = $derived(orchestrator.getSessions());
  const activeSessionId = $derived(orchestrator.getActiveSessionId());
  const projects = $derived(projectStore.getProjects());

  let archivedSessions = $state<Session[]>([]);
  let subMenu = $state<"none" | "archivedSessions" | "archiveProject" | "hideProject" | "unhideProject" | "deleteProject" | "restoreProject" | "pickTask" | "openFile" | "autoDispatch">("none");
  let archivedProjects = $state<Project[]>([]);
  let taskItems = $state<TaskItem[]>([]);
  let fileList = $state<string[]>([]);
  let projectAutoModes = $state<Record<string, boolean>>({});

  async function openArchived() {
    archivedSessions = await sessionsApi.listArchived();
    subMenu = "archivedSessions";
  }

  async function openArchivedProjects() {
    archivedProjects = await projectsApi.listArchived();
    subMenu = "restoreProject";
  }

  async function openTaskPicker() {
    const session = sessions.find((s) => s.id === activeSessionId);
    const project = session ? projects.find((p) => p.id === session.project_id) : projects[0];
    const repoPath = project?.path;
    if (!repoPath) { close(); return; }
    try {
      taskItems = await tasksApi.list(repoPath);
    } catch {
      taskItems = [];
    }
    subMenu = "pickTask";
  }

  function openArchiveProject() {
    subMenu = "archiveProject";
  }

  function openHideProject() {
    subMenu = "hideProject";
  }

  function openUnhideProject() {
    subMenu = "unhideProject";
  }

  async function updateProjectVisibility(
    id: string,
    action: (projectId: string) => void | Promise<void>,
    verb: "hide" | "unhide",
  ) {
    try {
      await action(id);
      close();
    } catch (error) {
      console.error(`Failed to ${verb} project:`, error);
      showSnackbar(`Failed to ${verb} project.`);
    }
  }

  export async function openFilePicker() {
    subMenu = "openFile";
    const repoPath = getActiveRootPath();
    if (!repoPath) { close(); return; }
    try {
      fileList = await git.listFiles(repoPath);
    } catch {
      fileList = [];
    }
  }

  function openDeleteProject() {
    subMenu = "deleteProject";
  }

  async function openAutoDispatch() {
    const modes: Record<string, boolean> = {};
    await Promise.all(projects.map(async (p) => {
      try { modes[p.id] = await projectsApi.getAutoMode(p.id); } catch { modes[p.id] = false; }
    }));
    projectAutoModes = modes;
    subMenu = "autoDispatch";
  }

  function projectName(projectId: string): string {
    return projects.find((p) => p.id === projectId)?.name ?? "unknown";
  }

  function close() {
    subMenu = "none";
    onOpenChange(false);
  }

  // Reset subMenu when dialog opens. If openFileMode, trigger file picker.
  let wasOpen = false;
  $effect(() => {
    const isOpen = open;
    if (isOpen && !wasOpen) {
      if (openFileMode) {
        openFilePicker();
      } else {
        subMenu = "none";
      }
    }
    wasOpen = isOpen;
  });

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
    <Dialog.Overlay class="fixed inset-0 z-50" />
    <Dialog.Content
      class="fixed left-1/2 top-1/2 z-50 w-[600px] -translate-x-1/2 -translate-y-1/2 rounded-xl border border-border-s bg-panel shadow-[0_26px_70px_-14px_rgba(0,0,0,0.6)] overflow-hidden"
    >
      <Dialog.Title class="sr-only">Command Menu</Dialog.Title>
      <Dialog.Description class="sr-only">Search sessions, archive, or create new.</Dialog.Description>
      {#if subMenu === "archivedSessions"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Search archived sessions..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">
                No archived sessions.
              </Command.Empty>
              <Command.Group>
              <Command.GroupHeading class="px-3 pb-1 pt-3 text-[10px] font-semibold text-t3 uppercase tracking-[.05em]">Archived Sessions</Command.GroupHeading>
                <Command.GroupItems>
                  {#each archivedSessions as session (session.id)}
                    <Command.Item
                      value="restore {session.name || session.branch} {projectName(session.project_id)}"
                      keywords={[session.name, session.branch, projectName(session.project_id)]}
                      class="flex h-9 cursor-pointer items-center justify-between rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={() => { onRestoreSession(session.id); archivedSessions = archivedSessions.filter(s => s.id !== session.id); }}
                    >
                      <span class="truncate">{session.name || session.branch} <span class="text-xs text-t3">({projectName(session.project_id)})</span></span>
                      <span class="text-xs text-accent shrink-0 ml-2">Restore</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
              <Command.Separator class="my-1 h-px bg-border" />
              <Command.Group>
                <Command.GroupHeading class="px-3 pb-1 pt-3 text-[10px] font-semibold text-t3 uppercase tracking-[.05em]">Delete Archived</Command.GroupHeading>
                <Command.GroupItems>
                  {#each archivedSessions as session (session.id)}
                    <Command.Item
                      value="delete {session.name || session.branch} {projectName(session.project_id)}"
                      keywords={[session.name, session.branch, "delete", "destroy"]}
                      class="flex h-9 cursor-pointer items-center justify-between rounded-lg px-3 text-[13px] text-status-exited data-selected:bg-accent-bg"
                      onSelect={() => { onDestroyArchivedSession(session.id); archivedSessions = archivedSessions.filter(s => s.id !== session.id); }}
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
      {:else if subMenu === "archiveProject"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Archive which project..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No projects.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each projects as project (project.id)}
                    <Command.Item
                      value="archive {project.name}"
                      class="flex h-9 cursor-pointer items-center rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={() => { onArchiveProject(project.id); close(); }}
                    >
                      {project.name}
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "hideProject"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Hide which project from the sidebar..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No visible projects.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each projects.filter(project => !project.hidden) as project (project.id)}
                    <Command.Item
                      value="hide {project.name}"
                      class="flex h-9 cursor-pointer items-center rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={() => updateProjectVisibility(project.id, onHideProject, "hide")}
                    >
                      {project.name}
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "unhideProject"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Unhide which project..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No hidden projects.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each projects.filter(project => project.hidden) as project (project.id)}
                    <Command.Item
                      value="unhide {project.name}"
                      class="flex h-9 cursor-pointer items-center justify-between rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={() => updateProjectVisibility(project.id, onUnhideProject, "unhide")}
                    >
                      <span>{project.name}</span>
                      <span class="text-xs text-accent shrink-0 ml-2">Unhide</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "deleteProject"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Delete which project..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No projects.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each projects as project (project.id)}
                    <Command.Item
                      value="delete {project.name}"
                      class="flex h-9 cursor-pointer items-center rounded-lg px-3 text-[13px] text-status-exited data-selected:bg-accent-bg"
                      onSelect={() => { onDeleteProject(project.id); close(); }}
                    >
                      {project.name}
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "restoreProject"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Restore which project..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No archived projects.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each archivedProjects as project (project.id)}
                    <Command.Item
                      value="restore {project.name}"
                      class="flex h-9 cursor-pointer items-center justify-between rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={() => { onRestoreProject(project.id); archivedProjects = archivedProjects.filter(p => p.id !== project.id); }}
                    >
                      <span>{project.name}</span>
                      <span class="text-xs text-accent shrink-0 ml-2">Restore</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "pickTask"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Search tasks..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No tasks found.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each taskItems as task (task.key)}
                    <Command.Item
                      value="{task.key}: {task.title}"
                      keywords={[task.key, task.title]}
                      class="flex h-9 cursor-pointer items-center rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={() => { onPickTask(task); close(); }}
                    >
                      <span class="font-medium font-mono text-accent mr-2">{task.key}</span>
                      <span class="truncate">{task.title}</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "openFile"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Open file..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No files found.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each fileList as file (file)}
                    <Command.Item
                      value={file}
                      class="flex h-9 cursor-pointer items-center rounded-lg px-3 text-[12.5px] font-mono text-t1 data-selected:bg-accent-bg"
                      onSelect={() => { onOpenFile?.(file); close(); }}
                    >
                      <span class="truncate">{file}</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else if subMenu === "autoDispatch"}
        <Command.Root class="flex flex-col" loop disablePointerSelection>
          <Command.Input
            class="h-[54px] w-full border-b border-border bg-transparent px-4 text-[13.5px] outline-none placeholder:text-t3"
            placeholder="Toggle auto-dispatch..."
          />
          <Command.List class="max-h-72 overflow-y-auto p-2">
            <Command.Viewport>
              <Command.Empty class="flex items-center justify-center py-6 text-[13px] text-t3">No projects.</Command.Empty>
              <Command.Group>
                <Command.GroupItems>
                  {#each projects as project (project.id)}
                    <Command.Item
                      value="{projectAutoModes[project.id] ? 'disable' : 'enable'} auto-dispatch {project.name}"
                      keywords={[project.name, "auto", "dispatch", "toggle"]}
                      class="flex h-9 cursor-pointer items-center justify-between rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                      onSelect={async () => { const next = !projectAutoModes[project.id]; await projectsApi.setAutoMode(project.id, next); projectAutoModes = { ...projectAutoModes, [project.id]: next }; }}
                    >
                      <span>{project.name}</span>
                      <span class="text-xs shrink-0 ml-2 {projectAutoModes[project.id] ? 'text-status-running' : 'text-t3'}">{projectAutoModes[project.id] ? '✓ On' : 'Off'}</span>
                    </Command.Item>
                  {/each}
                </Command.GroupItems>
              </Command.Group>
            </Command.Viewport>
          </Command.List>
        </Command.Root>
      {:else}
      <Command.Root class="flex flex-col" loop disablePointerSelection>
        <Command.Input
          class="h-11 w-full border-b border-border bg-transparent px-4 text-sm outline-none placeholder:text-t3"
          placeholder="Go to a session, task, or action…"
        />
        <Command.List class="max-h-72 overflow-y-auto p-2">
          <Command.Viewport>
            <Command.Empty class="flex items-center justify-center py-6 text-sm text-t2">
              No results found.
            </Command.Empty>

            <Command.Group>
              <Command.GroupHeading class="px-3 pb-1 pt-3 text-[10px] font-semibold text-t3 uppercase tracking-[.05em]">Sessions</Command.GroupHeading>
              <Command.GroupItems>
                {#each sessions as session (session.id)}
                  <Command.Item
                    value="session {session.name || session.branch} {session.id}"
                    keywords={[session.name, session.branch]}
                    class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg {session.id === activeSessionId ? 'font-medium' : ''}"
                    onSelect={() => { onSelectSession(session.id); close(); }}
                  >
                    <span class="truncate">{session.name || session.branch}</span>
                  </Command.Item>
                {/each}
              </Command.GroupItems>
            </Command.Group>

            <Command.Separator class="my-1 h-px bg-border" />

            <Command.Group>
              <Command.GroupHeading class="px-3 pb-1 pt-3 text-[10px] font-semibold text-t3 uppercase tracking-[.05em]">Actions</Command.GroupHeading>
              <Command.GroupItems>
                <Command.Item
                  value="copy project root path"
                  keywords={["copy", "path", "root", "directory", "cwd"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={copyRootPath}
                >
                  Copy project root path
                </Command.Item>
                <Command.Item
                  value="rename current session"
                  keywords={["rename", "name", "edit"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onRenameSession(); close(); }}
                >
                  Rename session
                </Command.Item>
                <Command.Item
                  value="archive current session"
                  keywords={["archive", "close", "stop"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onArchiveSession(); close(); }}
                >
                  Archive current session
                </Command.Item>
                <Command.Item
                  value="delete current session"
                  keywords={["delete", "destroy", "remove", "kill"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-status-exited data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onDeleteSession(); close(); }}
                >
                  Delete current session
                </Command.Item>
                <Command.Item
                  value="create new session"
                  keywords={["new", "create", "add"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={() => { onNewSession(); close(); }}
                >
                  New session
                </Command.Item>
                <Command.Item
                  value="pick task"
                  keywords={["task", "kanban", "issue", "ticket", "pick"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={openTaskPicker}
                >
                  Pick task…
                </Command.Item>
                <Command.Item
                  value="create task"
                  keywords={["task", "new", "add", "create", "ticket"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={() => { onCreateTask(); close(); }}
                >
                  Create task…
                </Command.Item>
                <Command.Item
                  value={getSettings().hide_done_tasks ? "show done tasks" : "hide done tasks"}
                  keywords={["done", "tasks", "hide", "show", "toggle", "completed"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={() => { updateSettings({ hide_done_tasks: !getSettings().hide_done_tasks }); close(); }}
                >
                  {getSettings().hide_done_tasks ? "Show done tasks" : "Hide done tasks"}
                </Command.Item>
                <Command.Item
                  value={getSettings().hide_empty_projects ? "show empty projects" : "hide empty projects"}
                  keywords={["empty", "projects", "hide", "show", "toggle", "inactive"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={() => { updateSettings({ hide_empty_projects: !getSettings().hide_empty_projects }); close(); }}
                >
                  {getSettings().hide_empty_projects ? "Show empty projects" : "Hide empty projects"}
                </Command.Item>
                <Command.Item
                  value="reset terminal"
                  keywords={["reset", "clear", "redraw", "refresh", "fix"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onResetTerminal(); close(); }}
                >
                  Reset terminal
                </Command.Item>
                <Command.Item
                  value="view branch diff"
                  keywords={["diff", "changes", "git", "review", "branch"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onToggleDiff(); close(); }}
                >
                  View branch diff
                </Command.Item>
                <Command.Item
                  value="review changes"
                  keywords={["review", "diff", "changes", "git"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onToggleDiff(); close(); }}
                >
                  Review Changes
                  <kbd class="ml-auto text-[10px] font-mono text-t3">{MOD_LABEL}⇧R</kbd>
                </Command.Item>
                {#if onSplitVertical}
                <Command.Item
                  value="split vertical"
                  keywords={["split", "pane", "vertical", "side", "divide"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onSplitVertical(); close(); }}
                >
                  Split Vertical
                  <kbd class="ml-auto text-[10px] font-mono text-t3">{MOD_LABEL}D</kbd>
                </Command.Item>
                {/if}
                {#if onSplitHorizontal}
                <Command.Item
                  value="split horizontal"
                  keywords={["split", "pane", "horizontal", "top", "bottom", "divide"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onSplitHorizontal(); close(); }}
                >
                  Split Horizontal
                  <kbd class="ml-auto text-[10px] font-mono text-t3">{MOD_LABEL}⇧D</kbd>
                </Command.Item>
                {/if}
                {#if onCloseSplit}
                <Command.Item
                  value="close split"
                  keywords={["close", "split", "pane", "merge"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={() => { onCloseSplit(); close(); }}
                >
                  Close Split
                  <kbd class="ml-auto text-[10px] font-mono text-t3">{MOD_LABEL}⇧W</kbd>
                </Command.Item>
                {/if}
                <Command.Item
                  value="pull request"
                  keywords={["pr", "pull request", "github", "link", "review", "create"]}
                  disabled={!activeSessionId}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={async () => { const s = sessions.find(x => x.id === activeSessionId); if (s?.pr_url) { openUrl(s.pr_url); close(); } else if (onCreatePr) { close(); onCreatePr(); } }}
                >
                  {sessions.find(x => x.id === activeSessionId)?.pr_url ? "View PR" : "Create PR"}
                </Command.Item>
                {#if onOpenLogViewer}
                <Command.Item
                  value="session log viewer"
                  keywords={["log", "replay", "dogfood", "debug", "ansi", "session logs"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={() => { onOpenLogViewer(); close(); }}
                >
                  Session log viewer <span class="text-xs opacity-50 ml-1">dogfood</span>
                </Command.Item>
                {/if}
                <Command.Item
                  value="archived sessions"
                  keywords={["archived", "restore", "old", "hidden"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={openArchived}
                >
                  Archived sessions
                </Command.Item>
                <Command.Item
                  value="hide project"
                  keywords={["hide", "sidebar", "project"]}
                  disabled={!projects.some(project => !project.hidden)}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={openHideProject}
                >
                  Hide project…
                </Command.Item>
                <Command.Item
                  value="unhide project"
                  keywords={["unhide", "show", "sidebar", "project"]}
                  disabled={!projects.some(project => project.hidden)}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={openUnhideProject}
                >
                  Unhide project…
                </Command.Item>
                <Command.Item
                  value="archive project"
                  keywords={["archive", "project"]}
                  disabled={projects.length === 0}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={openArchiveProject}
                >
                  Archive project…
                </Command.Item>
                <Command.Item
                  value="delete project"
                  keywords={["delete", "remove", "project"]}
                  disabled={projects.length === 0}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-status-exited data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={openDeleteProject}
                >
                  Delete project…
                </Command.Item>
                <Command.Item
                  value="restore project"
                  keywords={["restore", "unarchive", "project"]}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg"
                  onSelect={openArchivedProjects}
                >
                  Restore project…
                </Command.Item>
                <Command.Item
                  value="toggle auto-dispatch"
                  keywords={["auto", "dispatch", "toggle", "project", "automatic"]}
                  disabled={projects.length === 0}
                  class="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-3 text-[13px] text-t1 data-selected:bg-accent-bg aria-disabled:opacity-50 aria-disabled:cursor-not-allowed"
                  onSelect={openAutoDispatch}
                >
                  Toggle auto-dispatch…
                </Command.Item>
              </Command.GroupItems>
            </Command.Group>
          </Command.Viewport>
        </Command.List>
      </Command.Root>
      {/if}
      <!-- Footer hints -->
      <div class="h-[42px] flex items-center gap-4 px-4 border-t border-border bg-panel-hi text-[11px] text-t3">
        <span><kbd class="font-mono">↑↓</kbd> Navigate</span>
        <span><kbd class="font-mono">↵</kbd> Open</span>
        <span><kbd class="font-mono">esc</kbd> Dismiss</span>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
