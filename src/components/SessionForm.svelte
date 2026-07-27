<script lang="ts">
  import { sessions as sessionsApi, projects as projectsApi, tasks as tasksApi } from "../lib/api";
  import type { Session, Project, TaskItem } from "../lib/types";
  import { Button, Input, Label, Select, Checkbox } from "./ui";
  import { getSettings } from "../lib/settings.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { LoaderCircle } from "@lucide/svelte";

  interface TaskPrefill { key: string; title: string; description: string; branch: string; name: string; prompt: string; baseBranch?: string; }
  interface Props { projects: Project[]; sessions: Session[]; onCreated: (session: Session) => void; onCancel: () => void; taskPrefill?: TaskPrefill | null; currentProjectId?: string | null; }

  let { projects, sessions, onCreated, onCancel, taskPrefill = null, currentProjectId = null }: Props = $props();

  const config = $derived(getSettings());
  const providerKeys = $derived(Object.keys(config.providers));

  // svelte-ignore state_referenced_locally
  let mode = $state<"task" | "manual">(taskPrefill ? "task" : "manual");
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
  let baseBranchValue = $state(taskPrefill?.baseBranch ?? "");

  // Task picker state
  let taskItems = $state<TaskItem[]>([]);
  let taskSearchValue = $state("");

  // Clear task-related state when switching to manual mode
  $effect(() => {
    if (mode === "manual") {
      taskKey = "";
      taskPrompt = "";
      sessionName = "";
      branchValue = "";
      branchSearch = "";
      newBranchName = "";
    }
  });

  const selectedProject = $derived(projects.find((p) => p.id === projectValue));

  $effect(() => {
    if (selectedProject) {
      projectsApi.listBranches(selectedProject.path).then(
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
      const taskFn = taskPrefill?.key ? tasksApi.listAll : tasksApi.list;
      taskFn(selectedProject.path).then(
        (items) => {
          taskItems = items;
          if (taskPrefill?.key) {
            taskSearchValue = taskPrefill.key;
            if (items.some((t) => t.key === taskPrefill.key)) {
              onTaskSelected(taskPrefill.key);
            }
          }
        },
        (e) => { taskItems = []; showSnackbar(String(e)); },
      );
    }
  });

  const taskSelectItems = $derived(taskItems.map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` })));

  function renderTemplate(template: string, task: TaskItem): string {
    return template.replace(/\{(\w+)(?::(\w+))?\}/g, (_, varName, transform) => {
      let val = varName === "blocked_by" ? task.blocked_by?.join(", ") ?? "" : String((task as any)[varName] ?? "");
      if (varName === "parent_key" && !val) val = task.key;
      if (transform === "slug") return val.toLowerCase().replace(/[^\w]+/g, "-").replace(/^-|-$/g, "");
      if (transform === "lower") return val.toLowerCase();
      if (transform === "upper") return val.toUpperCase();
      return val;
    });
  }

  function getTaskManagerTemplates() {
    return config.task_management?.templates;
  }

  function onTaskSelected(key: string) {
    const task = taskItems.find((t) => t.key === key);
    if (!task) return;
    taskKey = task.key;
    taskSearchValue = task.key;
    const templates = getTaskManagerTemplates();
    sessionName = templates?.name ? renderTemplate(templates.name, task) : `${task.key}: ${task.title}`;
    taskPrompt = templates?.prompt ? renderTemplate(templates.prompt, task) : (task.description ? `Implement task ${task.key}: ${task.title}\n\n${task.description}` : `Implement task ${task.key}: ${task.title}`);
    const slugBranch = templates?.branch ? renderTemplate(templates.branch, task) : `${task.key.toLowerCase()}/${task.title.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-/]/g, "")}`;
    branchSearch = slugBranch;
    branchValue = slugBranch;
    newBranchName = slugBranch;
    baseBranchValue = task.base_branch;
  }

  const branch = $derived((branchValue || branchSearch).replace(/^remote:/, ""));
  const isNewBranch = $derived(branch !== "" && !branches.some((b) => b.value === branchValue || b.value === `remote:${branch}`));
  const defaultBranchName = $derived(sessionName.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-/]/g, ""));
  const worktreeBranch = $derived(newBranchName || defaultBranchName);
  const baseBranch = $derived(baseBranchValue || "main");

  const branchAlreadyUsed = $derived(
    !useWorktree && projectValue && branch && sessions.some(s => s.project_id === projectValue && s.status === "active" && s.branch === branch && !s.worktree_path)
  );

  let formEl: HTMLFormElement;
  let wrapperEl = $state<HTMLDivElement | null>(null);
  let error = $state("");

  $effect(() => { if (wrapperEl) wrapperEl.focus(); });

  const fk = createFormKeyboardController(
    () => [
      { key: "r", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='project'] input") ?? null },
      { key: "s", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='name'] input") ?? null },
      { key: "w", toggle: () => { useWorktree = !useWorktree; } },
      { key: "a", toggle: () => { autoApprove = !autoApprove; } },
      { key: "p", toggle: () => { const current = selectedProvider || config.default_provider; const idx = providerKeys.indexOf(current); selectedProvider = providerKeys[(idx + 1) % providerKeys.length]; }, shiftToggle: () => { const current = selectedProvider || config.default_provider; const idx = providerKeys.indexOf(current); selectedProvider = providerKeys[(idx - 1 + providerKeys.length) % providerKeys.length]; } },
      { key: "b", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='base'] input") ?? null },
      { key: "n", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='branch'] input") ?? null },
      { key: "m", toggle: () => { mode = "manual"; } },
      { key: "t", toggle: () => { mode = "task"; } },
    ],
    { wrapper: () => wrapperEl, onDismiss: onCancel },
  );

  const badge = $derived(fk.mode === "normal" ? "bg-accent-bg text-accent" : "bg-panel-hi text-t3");

  function metaEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); }
  }

  let submitting = $state(false);

  async function submit() {
    if (submitting) return;
    if (!selectedProject) { error = "Select a project."; return; }
    submitting = true;

    const taskKeyParam = taskKey || null;
    const taskPromptParam = taskPrompt || null;

    if (useWorktree) {
      if (!worktreeBranch) { error = "Enter a branch name."; submitting = false; return; }
      try {
        const session = await sessionsApi.launch({
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
        const session = await sessionsApi.launch({
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
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div bind:this={wrapperEl} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none" data-form-keyboard>
<form bind:this={formEl} class="px-5 pb-0 space-y-3" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <!-- Mode toggle -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="flex rounded-lg bg-panel-hi p-0.5" role="toolbar" tabindex="-1">
    <button
      type="button"
      tabindex={-1}
      class="flex-1 px-3 py-1.5 text-[12px] font-medium rounded-md transition-colors {mode === 'manual' ? 'bg-accent text-on-accent' : 'text-t2 hover:text-t1'}"
      onclick={() => (mode = "manual")}
    >Manual <span class="font-mono text-[10px] opacity-60">M</span></button>
    <button
      type="button"
      tabindex={-1}
      class="flex-1 px-3 py-1.5 text-[12px] font-medium rounded-md transition-colors {mode === 'task' ? 'bg-accent text-on-accent' : 'text-t2 hover:text-t1'}"
      onclick={() => (mode = "task")}
    >From task <span class="font-mono text-[10px] opacity-60">T</span></button>
  </div>

  <div class="space-y-1" data-field="project">
    <Label>Project <span class="font-mono text-[10px] px-1 rounded {badge}">R</span></Label>
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

  <div class="space-y-1" data-field="name">
    <Label>Name <span class="font-mono text-[10px] px-1 rounded {badge}">S</span></Label>
    <Input
      bind:value={sessionName}
      onkeydown={metaEnter}
      placeholder="My session..."
    />
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="flex flex-col gap-2 rounded-lg border border-border px-3 py-2 outline-none"
    role="group"
    tabindex="-1"
  >
    <div class="flex items-center gap-4">
      <Checkbox id="use-worktree" label="Worktree" bind:checked={useWorktree} tabindex={-1} />
      <span class="font-mono text-[10px] px-1 rounded {badge}">W</span>
      <Checkbox id="auto-approve" label="Auto-approve" bind:checked={autoApprove} tabindex={-1} />
      <span class="font-mono text-[10px] px-1 rounded {badge}">A</span>
    </div>
    {#if providerKeys.length > 1}
      <div class="flex items-center gap-2">
        <span class="text-[11px] text-t3">Provider</span>
        <span class="text-[12px] text-t1 font-medium">{selectedProvider || config.default_provider}</span>
        <span class="font-mono text-[10px] px-1 rounded {badge}">P</span>
      </div>
    {/if}
  </div>

  {#if useWorktree}
    <div class="space-y-1" data-field="base">
      <Label>Base branch <span class="font-mono text-[10px] px-1 rounded {badge}">B</span></Label>
      <Select items={branches} bind:value={baseBranchValue} onkeydown={metaEnter} placeholder="main" emptyText="No branches found" />
    </div>

    <div class="space-y-1" data-field="branch">
      <Label>New branch name <span class="font-mono text-[10px] px-1 rounded {badge}">N</span></Label>
      <Input
        bind:value={newBranchName}
        onkeydown={metaEnter}
        placeholder={defaultBranchName || "feat/my-feature"}
      />
      {#if worktreeBranch}
        <p class="text-xs text-t3">Branch: <span class="font-medium font-mono text-t1">{worktreeBranch}</span></p>
      {/if}
    </div>
  {:else}
    <div class="space-y-1" data-field="branch">
      <Label>Branch <span class="font-mono text-[10px] px-1 rounded {badge}">N</span></Label>
      <Select items={branches} bind:value={branchValue} onInput={(s) => { branchSearch = s; }} onkeydown={metaEnter} placeholder="main, feat/new-feature..." emptyText="No branches found" />
    </div>

    {#if isNewBranch && branch}
      <div class="space-y-1" data-field="base">
        <Label>Base branch <span class="font-mono text-[10px] px-1 rounded {badge}">B</span></Label>
        <Select items={branches} bind:value={baseBranchValue} onkeydown={metaEnter} placeholder="main" emptyText="No branches found" />
      </div>
      <p class="text-xs text-t3">Will create new branch: <span class="font-medium font-mono text-t1">{branch}</span> from <span class="font-medium font-mono text-t1">{baseBranch}</span></p>
    {/if}
  {/if}

  {#if branchAlreadyUsed}
    <p class="text-xs text-status-review">Another session is using this branch — switching branches will affect it.</p>
  {/if}

  {#if error}
    <p class="text-xs text-status-exited">{error}</p>
  {/if}

  <!-- Footer with mode indicator -->
  <div class="sticky bottom-0 bg-panel flex items-center justify-between pt-2 pb-4 border-t border-border mt-3">
    <div class="flex items-center gap-2">
      {#if fk.mode === "insert"}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-accent-bg text-accent font-medium">INSERT</span>
        <span class="text-[10px] text-t3">esc → normal mode</span>
      {:else}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
        <span class="text-[10px] text-t3">press a key to focus field</span>
      {/if}
    </div>
    <div class="flex gap-2">
      <Button type="button" onclick={onCancel}>Cancel</Button>
      <Button type="submit" variant="primary" disabled={submitting}>
        {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}Create session <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>{/if}
      </Button>
    </div>
  </div>
</form>
</div>
