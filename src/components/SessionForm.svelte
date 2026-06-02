<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Button, Input, Label, Select, Checkbox } from "./ui";
  import { getSettings } from "../lib/settings.svelte";

  interface Project { id: string; name: string; path: string; }
  interface Session { id: string; project_id: string; name: string; tmux_name: string | null; branch: string; status: string; created_at: string; worktree_path: string | null; backend: string; }
  interface TaskPrefill { key: string; title: string; description: string; branch: string; name: string; prompt: string; }
  interface Props { projects: Project[]; sessions: Session[]; onCreated: (session: Session) => void; onCancel: () => void; taskPrefill?: TaskPrefill | null; }

  let { projects, sessions, onCreated, onCancel, taskPrefill = null }: Props = $props();

  const config = $derived(getSettings());
  const providerKeys = $derived(Object.keys(config.providers));

  let sessionName = $state(taskPrefill?.name ?? "");
  let taskKey = $state(taskPrefill?.key ?? "");
  let taskPrompt = $state(taskPrefill?.prompt ?? "");
  let useWorktree = $state(false);
  let autoApprove = $state(true);
  let selectedProvider = $state("");
  let newBranchName = $state("");

  let projectValue = $state(projects[0]?.id ?? "");
  const projectItems = projects.map((p) => ({ value: p.id, label: p.name }));

  let branchValue = $state("");
  let branchSearch = $state("");
  let branches = $state<{ value: string; label: string }[]>([]);

  let baseBranchValue = $state("");

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

  function focusBranchInput() {
    requestAnimationFrame(() => {
      formEl?.querySelectorAll('input')?.[2]?.focus();
    });
  }

  function metaEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); }
  }

  async function submit() {
    if (!selectedProject) { error = "Select a project."; return; }

    const taskKeyParam = taskKey || null;
    const taskPromptParam = taskPrompt || null;

    if (useWorktree) {
      if (!worktreeBranch) { error = "Enter a branch name."; return; }
      try {
        const session = await invoke<Session>("launch_session", {
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch: worktreeBranch, isNewBranch: true,
          name: sessionName, useWorktree: true, baseBranch, autoApprove,
          provider: selectedProvider || config.default_provider,
          taskKey: taskKeyParam, taskPrompt: taskPromptParam,
        });
        onCreated(session);
      } catch (e) { error = String(e); }
    } else {
      if (!branch) { error = "Enter a branch name."; return; }
      try {
        const session = await invoke<Session>("launch_session", {
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch, isNewBranch, name: sessionName,
          useWorktree: false, baseBranch: isNewBranch ? baseBranch : null, autoApprove,
          provider: selectedProvider || config.default_provider,
          taskKey: taskKeyParam, taskPrompt: taskPromptParam,
        });
        onCreated(session);
      } catch (e) { error = String(e); }
    }
  }
</script>

<form bind:this={formEl} class="space-y-4" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <div class="space-y-1">
    <Label>Task key</Label>
    <Input
      bind:value={taskKey}
      onkeydown={metaEnter}
      placeholder="KAN-3..."
      autocomplete="off"
    />
  </div>

  <div class="space-y-1">
    <Label>Name</Label>
    <Input
      bind:value={sessionName}
      onkeydown={metaEnter}
      placeholder="My session..."
      autocomplete="off"
    />
  </div>

  <div class="space-y-1">
    <Label>Project</Label>
    <Select items={projectItems} bind:value={projectValue} onValueChange={focusBranchInput} onkeydown={metaEnter} placeholder="Search project..." emptyText="No projects found" />
  </div>

  <div
    class="flex flex-col gap-2 rounded border border-transparent px-2 py-1.5 focus:border-surface-300 focus:bg-surface-50 dark:focus:border-surface-600 dark:focus:bg-surface-900 outline-none"
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
        autocomplete="off"
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
    <Button type="submit" variant="primary">Launch</Button>
  </div>
</form>
