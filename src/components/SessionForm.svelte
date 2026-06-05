<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Button, Input, Label, Select, Checkbox } from "./ui";
  import { getSettings } from "../lib/settings.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";

  interface Project { id: string; name: string; path: string; }
  interface Session { id: string; project_id: string; name: string; tmux_name: string | null; branch: string; status: string; created_at: string; worktree_path: string | null; backend: string; tab_count: number; base_branch: string | null; task_key: string | null; }
  interface TaskItem { key: string; title: string; status: string; description: string; priority: number; blocked_by: string[]; }
  interface TaskPrefill { key: string; title: string; description: string; branch: string; name: string; prompt: string; }
  interface Props { projects: Project[]; sessions: Session[]; onCreated: (session: Session) => void; onCancel: () => void; taskPrefill?: TaskPrefill | null; currentProjectId?: string | null; }

  let { projects, sessions, onCreated, onCancel, taskPrefill = null, currentProjectId = null }: Props = $props();

  const config = $derived(getSettings());
  const providerKeys = $derived(Object.keys(config.providers));
  const hasTaskManager = $derived(Object.keys(config.task_managers ?? {}).length > 0);

  // svelte-ignore state_referenced_locally
  let mode = $state<"task" | "manual">(taskPrefill ? "task" : (hasTaskManager ? "task" : "manual"));
  // svelte-ignore state_referenced_locally
  let sessionName = $state(taskPrefill?.name ?? "");
  // svelte-ignore state_referenced_locally
  let taskKey = $state(taskPrefill?.key ?? "");
  // svelte-ignore state_referenced_locally
  let taskPrompt = $state(taskPrefill?.prompt ?? "");
  let useWorktree = $state(false);
  let autoApprove = $state(true);
  let selectedProvider = $state("");
  let newBranchName = $state("");

  // svelte-ignore state_referenced_locally
  let projectValue = $state(currentProjectId ?? projects[0]?.id ?? "");
  // svelte-ignore state_referenced_locally
  const projectItems = projects.map((p) => ({ value: p.id, label: p.name }));

  let branchValue = $state("");
  let branchSearch = $state("");
  let branches = $state<{ value: string; label: string }[]>([]);
  let baseBranchValue = $state("");

  // Task picker state
  let taskItems = $state<TaskItem[]>([]);
  let taskSearchValue = $state("");

  const selectedProject = $derived(projects.find((p) => p.id === projectValue));

  $effect(() => {
    if (selectedProject) {
      invoke<string[]>("list_branches", { repoPath: selectedProject.path }).then(
        (b) => (branches = b.map((s) => {
          const remote = s.startsWith("remote:");
          const name = remote ? s.slice(7) : s;
          return { value: remote ? `remote:${name}` : name, label: name, remote };
        })),
        () => (branches = []),
      );
    }
  });

  // Fetch tasks when in task mode and project changes
  $effect(() => {
    if (mode === "task" && selectedProject) {
      invoke<TaskItem[]>("list_task_items", { repoPath: selectedProject.path }).then(
        (items) => (taskItems = items),
        (e) => { taskItems = []; showSnackbar(String(e)); },
      );
    }
  });

  const taskSelectItems = $derived(taskItems.map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` })));

  function renderTemplate(template: string, task: TaskItem): string {
    return template.replace(/\{(\w+)(?::(\w+))?\}/g, (_, varName, transform) => {
      const val = varName === "blocked_by" ? task.blocked_by?.join(", ") ?? "" : String((task as any)[varName] ?? "");
      if (transform === "slug") return val.toLowerCase().replace(/[^\w]+/g, "-").replace(/^-|-$/g, "");
      if (transform === "lower") return val.toLowerCase();
      if (transform === "upper") return val.toUpperCase();
      return val;
    });
  }

  function getTaskManagerTemplates() {
    const tms = config.task_managers ?? {};
    const tmKey = config.default_task_manager || Object.keys(tms)[0];
    return tms[tmKey]?.templates;
  }

  function onTaskSelected(key: string) {
    const task = taskItems.find((t) => t.key === key);
    if (!task) return;
    taskKey = task.key;
    const templates = getTaskManagerTemplates();
    sessionName = templates?.name ? renderTemplate(templates.name, task) : `${task.key}: ${task.title}`;
    taskPrompt = templates?.prompt ? renderTemplate(templates.prompt, task) : (task.description ? `Implement task ${task.key}: ${task.title}\n\n${task.description}` : `Implement task ${task.key}: ${task.title}`);
    const slugBranch = templates?.branch ? renderTemplate(templates.branch, task) : `${task.key.toLowerCase()}/${task.title.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-\/]/g, "")}`;
    branchSearch = slugBranch;
    branchValue = slugBranch;
    newBranchName = slugBranch;
  }

  const branch = $derived((branchValue || branchSearch).replace(/^remote:/, ""));
  const isNewBranch = $derived(branch !== "" && !branches.some((b) => b.value === branchValue || b.value === `remote:${branch}`));
  const defaultBranchName = $derived(sessionName.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-\/]/g, ""));
  const worktreeBranch = $derived(newBranchName || defaultBranchName);
  const baseBranch = $derived(baseBranchValue || "main");

  const branchAlreadyUsed = $derived(
    !useWorktree && projectValue && branch && sessions.some(s => s.project_id === projectValue && s.status === "active" && s.branch === branch && !s.worktree_path)
  );

  let formEl: HTMLFormElement;
  let error = $state("");

  function metaEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); }
  }

  function formKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; }
    if (!hasTaskManager) return;
    const el = document.activeElement;
    if (el && (el.tagName === "INPUT" || el.closest("[role='combobox']"))) return;
    if (e.key === "t") { e.preventDefault(); mode = "task"; }
    if (e.key === "m") { e.preventDefault(); mode = "manual"; }
  }

  let submitting = false;

  async function submit() {
    if (submitting) return;
    if (!selectedProject) { error = "Select a project."; return; }
    submitting = true;

    const taskKeyParam = taskKey || null;
    const taskPromptParam = taskPrompt || null;

    if (useWorktree) {
      if (!worktreeBranch) { error = "Enter a branch name."; submitting = false; return; }
      try {
        const session = await invoke<Session>("launch_session", {
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch: worktreeBranch, isNewBranch: true,
          name: sessionName, useWorktree: true, baseBranch, autoApprove,
          provider: selectedProvider || config.default_provider,
          taskKey: taskKeyParam, taskPrompt: taskPromptParam,
        });
        onCreated(session);
      } catch (e) { error = String(e); submitting = false; }
    } else {
      if (!branch) { error = "Enter a branch name."; submitting = false; return; }
      try {
        const session = await invoke<Session>("launch_session", {
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch, isNewBranch, name: sessionName,
          useWorktree: false, baseBranch: isNewBranch ? baseBranch : null, autoApprove,
          provider: selectedProvider || config.default_provider,
          taskKey: taskKeyParam, taskPrompt: taskPromptParam,
        });
        onCreated(session);
      } catch (e) { error = String(e); submitting = false; }
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form bind:this={formEl} class="space-y-4" onsubmit={(e) => { e.preventDefault(); submit(); }} onkeydown={formKeydown}>
  <!-- Mode toggle -->
  {#if hasTaskManager}
  <!-- svelte-ignore a11y_autofocus -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="flex rounded-md border border-surface-200 dark:border-surface-700 overflow-hidden" role="toolbar" tabindex="0" autofocus onkeydown={(e) => { if (e.key === "t") { e.preventDefault(); mode = "task"; } if (e.key === "m") { e.preventDefault(); mode = "manual"; } }}>
    <button
      type="button"
      tabindex={-1}
      class="flex-1 px-3 py-1.5 text-sm font-medium transition-colors {mode === 'task' ? 'bg-primary-500 text-primary-50' : 'bg-surface-100 dark:bg-surface-800 text-surface-600 dark:text-surface-400 hover:bg-surface-200 dark:hover:bg-surface-700'}"
      onclick={() => (mode = "task")}
    >From task <span class="text-[10px] opacity-70">T</span></button>
    <button
      type="button"
      tabindex={-1}
      class="flex-1 px-3 py-1.5 text-sm font-medium transition-colors {mode === 'manual' ? 'bg-primary-500 text-primary-50' : 'bg-surface-100 dark:bg-surface-800 text-surface-600 dark:text-surface-400 hover:bg-surface-200 dark:hover:bg-surface-700'}"
      onclick={() => (mode = "manual")}
    >Manual <span class="text-[10px] opacity-70">M</span></button>
  </div>
  {/if}

  <div class="space-y-1">
    <Label>Project</Label>
    <Select items={projectItems} bind:value={projectValue} onkeydown={metaEnter} placeholder="Search project..." emptyText="No projects found" />
  </div>

  <!-- Task picker (From task mode) -->
  {#if mode === "task"}
    <div class="space-y-1">
      <Label>Task</Label>
      <Select
        items={taskSelectItems}
        bind:value={taskSearchValue}
        onValueChange={onTaskSelected}
        onkeydown={metaEnter}
        placeholder="Search tasks..."
        emptyText="No tasks found"
      />
    </div>
  {/if}

  <div class="space-y-1">
    <Label>Name</Label>
    <Input
      bind:value={sessionName}
      onkeydown={metaEnter}
      placeholder="My session..."
    />
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="flex flex-col gap-2 rounded border border-transparent px-2 py-1.5 focus:border-surface-300 focus:bg-surface-50 dark:focus:border-surface-600 dark:focus:bg-surface-900 outline-none"
    role="group"
    tabindex="0"
    onkeydown={(e) => {
      if (e.key === "w") { e.preventDefault(); useWorktree = !useWorktree; }
      if (e.key === "a") { e.preventDefault(); autoApprove = !autoApprove; }
      if (e.key === "p" && providerKeys.length > 1) {
        e.preventDefault();
        const current = selectedProvider || config.default_provider;
        const idx = providerKeys.indexOf(current);
        selectedProvider = providerKeys[(idx + 1) % providerKeys.length];
      }
    }}
  >
    <div class="flex items-center gap-4">
      <Checkbox id="use-worktree" label="Worktree" bind:checked={useWorktree} tabindex={-1} />
      <span class="text-[10px] text-surface-600 dark:text-surface-400">W</span>
      <Checkbox id="auto-approve" label="Auto-approve" bind:checked={autoApprove} tabindex={-1} />
      <span class="text-[10px] text-surface-600 dark:text-surface-400">A</span>
    </div>
    {#if providerKeys.length > 1}
      <div class="flex items-center gap-2">
        <span class="text-[10px] text-surface-500 dark:text-surface-400">Provider</span>
        <span class="text-xs text-surface-700 dark:text-surface-300 font-medium">{selectedProvider || config.default_provider}</span>
        <span class="text-[10px] text-surface-600 dark:text-surface-400">P</span>
      </div>
    {/if}
  </div>

  {#if useWorktree}
    <div class="space-y-1">
      <Label>Base branch</Label>
      <Select items={branches} bind:value={baseBranchValue} onkeydown={metaEnter} placeholder="main" emptyText="No branches found" />
    </div>

    <div class="space-y-1">
      <Label>New branch name</Label>
      <Input
        bind:value={newBranchName}
        onkeydown={metaEnter}
        placeholder={defaultBranchName || "feat/my-feature"}
      />
      {#if worktreeBranch}
        <p class="text-xs text-surface-500">Branch: <span class="font-medium text-surface-900 dark:text-surface-100">{worktreeBranch}</span></p>
      {/if}
    </div>
  {:else}
    <div class="space-y-1">
      <Label>Branch</Label>
      <Select items={branches} bind:value={branchValue} onInput={(s) => { branchSearch = s; }} onkeydown={metaEnter} placeholder="main, feat/new-feature..." emptyText="No branches found" />
    </div>

    {#if isNewBranch && branch}
      <div class="space-y-1">
        <Label>Base branch</Label>
        <Select items={branches} bind:value={baseBranchValue} onkeydown={metaEnter} placeholder="main" emptyText="No branches found" />
      </div>
      <p class="text-xs text-surface-500">Will create new branch: <span class="font-medium text-surface-900 dark:text-surface-100">{branch}</span> from <span class="font-medium text-surface-900 dark:text-surface-100">{baseBranch}</span></p>
    {/if}
  {/if}

  {#if branchAlreadyUsed}
    <p class="text-xs text-warning-500">Another session is using this branch — switching branches will affect it.</p>
  {/if}

  {#if error}
    <p class="text-xs text-error-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <Button type="button" onclick={onCancel}>Cancel</Button>
    <Button type="submit" variant="primary">Launch <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span></Button>
  </div>
</form>
