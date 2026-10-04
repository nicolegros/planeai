<script lang="ts">
  import { sessions as sessionsApi, projects as projectsApi, tasks as tasksApi } from "../lib/api";
  import { sessionTaskProjectId, type Session, type Project, type TaskItem } from "../lib/types";
  import { Button, Input, Label, Select, Checkbox } from "./ui";
  import { getSettings } from "../lib/settings.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { renderTemplate } from "../lib/render-template";
  import { LoaderCircle } from "@lucide/svelte";
  import type { RuntimeProvider } from "../lib/plugin-providers";
  import { ProviderChoice } from "../lib/provider-choice.svelte";

  interface TaskPrefill { key: string; title: string; description: string; branch: string; name: string; prompt: string; baseBranch?: string; projectId?: string | null; }
  interface Props {
    projects: Project[];
    sessions: Session[];
    onCreated: (session: Session) => void;
    onCancel: () => void;
    onCreateTask: () => void;
    taskPrefill?: TaskPrefill | null;
    currentProjectId?: string | null;
    /** Providers contributed by running plugins, offered after the configured ones. */
    runtimeProviders?: RuntimeProvider[];
  }

  let { projects, sessions, onCreated, onCancel, onCreateTask, taskPrefill = null, currentProjectId = null, runtimeProviders = [] }: Props = $props();

  const config = $derived(getSettings());
  const providers = new ProviderChoice(() => runtimeProviders);

  let mode = $state<"task">("task");
  // svelte-ignore state_referenced_locally
  let sessionName = $state(taskPrefill?.name ?? "");
  /** A name typed before the prefilled task loads must survive that load. */
  let nameEdited = false;
  // svelte-ignore state_referenced_locally
  let taskKey = $state(taskPrefill?.key ?? "");
  // svelte-ignore state_referenced_locally
  let taskPrompt = $state(taskPrefill?.prompt ?? "");
  let useWorktree = $state(false);
  let autoApprove = $state(true);
  let newBranchName = $state("");

  // svelte-ignore state_referenced_locally
  let projectValue = $state(currentProjectId ?? projects[0]?.id ?? "");
  // svelte-ignore state_referenced_locally
  const projectItems = projects.map((p) => ({ value: p.id, label: p.name }));
  /** Project owning the linked task; the session may run in another one. */
  // svelte-ignore state_referenced_locally
  let taskProjectId = $state(taskPrefill?.projectId ?? projectValue);
  /** Kept across project changes so the task stays linked when the picker lists another project. */
  let linkedTask = $state<TaskItem | null>(null);
  const isCrossProject = $derived(!!linkedTask && taskProjectId !== projectValue);
  const taskProjectName = $derived(projects.find((p) => p.id === taskProjectId)?.name ?? "");

  let branchValue = $state("");
  let branchSearch = $state("");
  let branches = $state<{ value: string; label: string }[]>([]);
  /** Project the current `branches` and `defaultBranch` were listed for. */
  let branchesProjectId = $state("");
  let defaultBranch = $state("");
  // svelte-ignore state_referenced_locally
  let baseBranchValue = $state(taskPrefill?.baseBranch ?? "");
  // svelte-ignore state_referenced_locally
  let taskBaseBranch = $state(taskPrefill?.baseBranch ?? "");
  /** A base branch picked by hand is never replaced by the task's. */
  let baseBranchPicked = $state(false);

  // Task picker state
  let taskItems = $state<TaskItem[]>([]);
  let taskSearchValue = $state("");
  let prefillApplied = false;

  const selectedProject = $derived(projects.find((p) => p.id === projectValue));

  $effect(() => {
    const project = selectedProject;
    if (!project) return;
    const stale = () => selectedProject?.id !== project.id;
    Promise.all([
      projectsApi.listBranches(project.path),
      projectsApi.detectDefaultBranch(project.path).catch(() => ""),
    ]).then(
      ([names, detected]) => {
        if (stale()) return;
        branches = names.map((s) => {
          const remote = s.startsWith("remote:");
          const name = remote ? s.slice(7) : s;
          return { value: remote ? `remote:${name}` : name, label: name, remote };
        });
        defaultBranch = detected;
        branchesProjectId = project.id;
      },
      // Without a branch list nothing is known to be missing, so the task's base is kept.
      () => {
        if (stale()) return;
        branches = [];
        defaultBranch = "";
        branchesProjectId = "";
      },
    );
  });

  function hasBranch(name: string): boolean {
    return branches.some((b) => b.value === name || b.value === `remote:${name}`);
  }

  // Another repo may lack the task's base branch; fall back to that repo's default, if it has one.
  const taskBaseMissing = $derived(
    isCrossProject && !!taskBaseBranch && branchesProjectId === projectValue && !hasBranch(taskBaseBranch),
  );
  const fallbackBaseBranch = $derived(defaultBranch && hasBranch(defaultBranch) ? defaultBranch : "");

  $effect(() => {
    if (baseBranchPicked || !taskBaseBranch || branchesProjectId !== projectValue) return;
    baseBranchValue = taskBaseMissing ? fallbackBaseBranch : taskBaseBranch;
  });

  // Fetch tasks when in task mode and project changes
  $effect(() => {
    if (mode === "task" && selectedProject) {
      const project = selectedProject;
      const taskFn = taskPrefill?.key ? tasksApi.listAll : tasksApi.list;
      taskFn(project.path).then(
        (items) => {
          if (selectedProject?.id === project.id) taskItems = items;
          // The prefilled task links from its own project's listing, even after a project switch.
          if (taskPrefill?.key && !prefillApplied && project.id === taskProjectId) {
            prefillApplied = true;
            taskSearchValue = taskPrefill.key;
            const task = items.find((t) => t.key === taskPrefill.key);
            if (task) linkTask(task, project.id, { keepTypedName: true });
          }
        },
        (e) => { if (selectedProject?.id === project.id) taskItems = []; showSnackbar(String(e)); },
      );
    }
  });

  const taskSelectItems = $derived([
    ...(isCrossProject && linkedTask ? [{ value: linkedTask.key, label: `${linkedTask.key}: ${linkedTask.title} · ${taskProjectName}` }] : []),
    ...taskItems.map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` })),
  ]);

  function getTaskManagerTemplates() {
    return config.task_management?.templates;
  }

  function onProjectChanged() {
    if (linkedTask) applyTaskDefaults(linkedTask, { keepTypedName: true });
  }

  function onTaskSelected(key: string) {
    const listed = taskItems.find((t) => t.key === key);
    if (listed) linkTask(listed, projectValue);
    else if (linkedTask?.key === key) applyTaskDefaults(linkedTask);
  }

  function linkTask(task: TaskItem, ownerProjectId: string, { keepTypedName = false } = {}) {
    taskProjectId = ownerProjectId;
    linkedTask = task;
    taskBaseBranch = task.base_branch;
    baseBranchPicked = false;
    applyTaskDefaults(task, { keepTypedName });
  }

  function applyTaskDefaults(task: TaskItem, { keepTypedName = false } = {}) {
    taskKey = task.key;
    taskSearchValue = task.key;
    const taskSessions = sessions.filter((session) => session.task_key === task.key && sessionTaskProjectId(session) === taskProjectId);
    // The name counts every agent in the task workspace; branches and checkouts only clash within one repo.
    const agentOrdinal = taskSessions.length + 1;
    const repoOrdinal = taskSessions.filter((session) => session.project_id === projectValue).length + 1;
    const templates = getTaskManagerTemplates();
    if (!(keepTypedName && nameEdited)) sessionName = agentOrdinal === 1 ? task.title : `${task.title} (${agentOrdinal})`;
    taskPrompt = templates?.prompt ? renderTemplate(templates.prompt, task) : (task.description ? `Implement task ${task.key}: ${task.title}\n\n${task.description}` : `Implement task ${task.key}: ${task.title}`);
    const baseTaskBranch = templates?.branch ? renderTemplate(templates.branch, task) : `${task.key.toLowerCase()}/${task.title.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-/]/g, "")}`;
    const taskBranch = repoOrdinal === 1 ? baseTaskBranch : `${baseTaskBranch}--${repoOrdinal}`;
    branchSearch = taskBranch;
    branchValue = taskBranch;
    newBranchName = taskBranch;
    // Additional agents in the same repo are isolated by default. Users can explicitly
    // disable this and reuse a checkout/worktree through the existing form controls.
    useWorktree = repoOrdinal > 1;
    if (!baseBranchPicked) baseBranchValue = taskBaseBranch;
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

  const fk = createFormKeyboardController(
    () => [
      { key: "r", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='project'] input") ?? null },
      { key: "t", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='task'] input") ?? null },
      { key: "m", toggle: onCreateTask },
      { key: "s", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='name'] input") ?? null },
      { key: "w", toggle: () => { useWorktree = !useWorktree; } },
      { key: "a", toggle: () => { if (!providers.autoApproveBlocked) autoApprove = !autoApprove; } },
      { key: "p", toggle: () => providers.cycle(1), shiftToggle: () => providers.cycle(-1) },
      { key: "b", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='base'] input") ?? null },
      { key: "n", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='branch'] input") ?? null },
    ],
    // svelte-ignore state_referenced_locally
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
    if (!taskKey) { error = "Select a task."; return; }
    if ((useWorktree || isNewBranch) && !baseBranchValue && taskBaseMissing) { error = "Select a base branch."; return; }
    submitting = true;

    const taskKeyParam = taskKey || null;
    const taskPromptParam = taskPrompt || null;
    const taskProjectIdParam = taskProjectId !== selectedProject.id ? taskProjectId : null;

    if (useWorktree) {
      if (!worktreeBranch) { error = "Enter a branch name."; submitting = false; return; }
      try {
        const { session, warning } = await sessionsApi.launch({
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch: worktreeBranch, isNewBranch: true,
          name: sessionName, useWorktree: true, baseBranch, autoApprove: providers.autoApprove(autoApprove),
          provider: providers.key,
          taskKey: taskKeyParam, taskProjectId: taskProjectIdParam, taskPrompt: taskPromptParam,
        });
        if (warning) showSnackbar(warning, "success");
        onCreated(session);
      } catch (e) { error = String(e); submitting = false; }
    } else {
      if (!branch) { error = "Enter a branch name."; submitting = false; return; }
      try {
        const { session, warning } = await sessionsApi.launch({
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch, isNewBranch, name: sessionName,
          useWorktree: false, baseBranch: isNewBranch ? baseBranch : null, autoApprove: providers.autoApprove(autoApprove),
          provider: providers.key,
          taskKey: taskKeyParam, taskProjectId: taskProjectIdParam, taskPrompt: taskPromptParam,
        });
        if (warning) showSnackbar(warning, "success");
        onCreated(session);
      } catch (e) { error = String(e); submitting = false; }
    }
  }
</script>

{#snippet baseFallbackHint()}
  <p class="text-xs text-t3">Task base <span class="font-medium font-mono text-t1">{taskBaseBranch}</span> not found in {selectedProject?.name}, {#if fallbackBaseBranch}using <span class="font-medium font-mono text-t1">{fallbackBaseBranch}</span>{:else}pick a base branch{/if}</p>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div bind:this={wrapperEl} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none" data-form-keyboard>
<form bind:this={formEl} class="px-5 pb-0 space-y-3" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <!-- Task is mandatory; create one through the existing TaskForm when needed. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="flex rounded-lg bg-panel-hi p-0.5" role="toolbar" tabindex="-1">
    <button
      type="button"
      tabindex={-1}
      class="flex-1 px-3 py-1.5 text-[12px] font-medium rounded-md transition-colors bg-accent text-on-accent"
      onclick={onCreateTask}
    >New task <span class="font-mono text-[10px] opacity-60">M</span></button>
  </div>

  <div class="space-y-1" data-field="project">
    <Label>Project <span class="font-mono text-[10px] px-1 rounded {badge}">R</span></Label>
    <Select items={projectItems} bind:value={projectValue} onValueChange={onProjectChanged} onkeydown={metaEnter} placeholder="Search project..." emptyText="No projects found" />
  </div>

  <div class="space-y-1" data-field="task">
    <Label>Task <span class="font-mono text-[10px] px-1 rounded {badge}">T</span></Label>
    <Select
      items={taskSelectItems}
      bind:value={taskSearchValue}
      onValueChange={(key) => onTaskSelected(key)}
      onkeydown={metaEnter}
      placeholder="Search tasks..."
      emptyText="No tasks found"
    />
    {#if isCrossProject}
      <p class="text-xs text-t3">Linked to task in <span class="font-medium text-t1">{taskProjectName}</span></p>
    {/if}
  </div>

  <div class="space-y-1" data-field="name">
    <Label>Name <span class="font-mono text-[10px] px-1 rounded {badge}">S</span></Label>
    <Input
      bind:value={sessionName}
      oninput={() => (nameEdited = true)}
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
      <Checkbox id="auto-approve" label="Auto-approve" bind:checked={() => providers.autoApprove(autoApprove), (value) => (autoApprove = value)} disabled={!!providers.autoApproveBlocked} title={providers.autoApproveBlocked} tabindex={-1} />
      <span class="font-mono text-[10px] px-1 rounded {badge}">A</span>
    </div>
    {#if providers.keys.length > 1}
      <div class="flex items-center gap-2">
        <span class="text-[11px] text-t3">Provider</span>
        <span class="text-[12px] text-t1 font-medium">{providers.label(providers.key)}</span>
        <span class="font-mono text-[10px] px-1 rounded {badge}">P</span>
      </div>
    {/if}
  </div>

  {#if useWorktree}
    <div class="space-y-1" data-field="base">
      <Label>Base branch <span class="font-mono text-[10px] px-1 rounded {badge}">B</span></Label>
      <Select items={branches} bind:value={baseBranchValue} onValueChange={() => (baseBranchPicked = true)} onkeydown={metaEnter} placeholder="main" emptyText="No branches found" />
      {#if taskBaseMissing && !baseBranchPicked}{@render baseFallbackHint()}{/if}
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
        <Select items={branches} bind:value={baseBranchValue} onValueChange={() => (baseBranchPicked = true)} onkeydown={metaEnter} placeholder="main" emptyText="No branches found" />
      {#if taskBaseMissing && !baseBranchPicked}{@render baseFallbackHint()}{/if}
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
    <div class="flex items-center gap-2" role="status" aria-live="polite">
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
