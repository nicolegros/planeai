<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Combobox } from "bits-ui";
  import { Button, Input, Label, Checkbox } from "./ui";
  import { getSettings } from "../lib/settings.svelte";

  interface Project { id: string; name: string; path: string; }
  interface Session { id: string; project_id: string; name: string; tmux_name: string; branch: string; status: string; created_at: string; worktree_path: string | null; }
  interface Props { projects: Project[]; onCreated: (session: Session) => void; onCancel: () => void; }

  let { projects, onCreated, onCancel }: Props = $props();

  const config = $derived(getSettings());
  const providerKeys = $derived(Object.keys(config.providers));

  let sessionName = $state("");
  let useWorktree = $state(false);
  let autoApprove = $state(true);
  let selectedProvider = $state("");
  let baseBranchValue = $state("");
  let baseBranchSearch = $state("");
  let newBranchName = $state("");

  let projectValue = $state(projects[0]?.id ?? "");
  let projectSearch = $state("");
  const projectItems = projects.map((p) => ({ value: p.id, label: p.name }));
  const filteredProjects = $derived(
    projectSearch === "" ? projectItems : projectItems.filter((p) => p.label.toLowerCase().includes(projectSearch.toLowerCase())),
  );

  let branchValue = $state("");
  let branchSearch = $state("");
  let branches = $state<{ value: string; label: string }[]>([]);

  const selectedProject = $derived(projects.find((p) => p.id === projectValue));

  $effect(() => {
    if (selectedProject) {
      invoke<string[]>("list_branches", { repoPath: selectedProject.path }).then(
        (b) => (branches = b.map((name) => ({ value: name, label: name }))),
        () => (branches = []),
      );
    }
  });

  const filteredBranches = $derived(
    branchSearch === "" ? branches : branches.filter((b) => b.label.toLowerCase().includes(branchSearch.toLowerCase())),
  );

  const filteredBaseBranches = $derived(
    baseBranchSearch === "" ? branches : branches.filter((b) => b.label.toLowerCase().includes(baseBranchSearch.toLowerCase())),
  );

  const branch = $derived(branchValue || branchSearch);
  const isNewBranch = $derived(branch !== "" && !branches.some((b) => b.value === branch));

  const defaultBranchName = $derived(sessionName.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-\/]/g, ""));
  const worktreeBranch = $derived(newBranchName || defaultBranchName);
  const baseBranch = $derived(baseBranchValue || baseBranchSearch || "main");

  let formEl: HTMLFormElement;
  let error = $state("");

  const comboInputClass = "w-full rounded border border-surface-300 bg-surface-50 px-3 py-2 text-sm text-surface-900 placeholder:text-surface-400 dark:border-surface-600 dark:bg-surface-900 dark:text-surface-50 dark:placeholder:text-surface-500";
  const comboContentClass = "z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-surface-200 bg-surface-50 shadow-lg dark:border-surface-700 dark:bg-surface-900";
  const comboItemClass = "cursor-pointer px-3 py-2 text-sm text-surface-700 data-[highlighted]:bg-surface-100 dark:text-surface-300 dark:data-[highlighted]:bg-surface-800";

  function focusBranchInput() {
    requestAnimationFrame(() => {
      formEl?.querySelectorAll('input')?.[2]?.focus();
    });
  }

  async function submit() {
    if (!selectedProject) { error = "Select a project."; return; }

    if (useWorktree) {
      if (!worktreeBranch) { error = "Enter a branch name."; return; }
      try {
        const session = await invoke<Session>("launch_session", {
          projectId: selectedProject.id, projectName: selectedProject.name,
          repoPath: selectedProject.path, branch: worktreeBranch, isNewBranch: true,
          name: sessionName, useWorktree: true, baseBranch, autoApprove,
          provider: selectedProvider || config.default_provider,
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
        });
        onCreated(session);
      } catch (e) { error = String(e); }
    }
  }
</script>

<form bind:this={formEl} class="space-y-4" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <div class="space-y-1">
    <Label>Name</Label>
    <Input
      bind:value={sessionName}
      onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
      placeholder="My session..."
      autocomplete="off"
    />
  </div>

  <div class="space-y-1">
    <Label>Project</Label>
    <Combobox.Root type="single" bind:value={projectValue} onValueChange={focusBranchInput} onOpenChangeComplete={(o) => { if (!o) projectSearch = ""; }}>
      <Combobox.Input
        oninput={(e) => (projectSearch = e.currentTarget.value)}
        onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
        placeholder="Search project..."
        autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other"
        class={comboInputClass}
      />
      <Combobox.Portal>
        <Combobox.Content class={comboContentClass} sideOffset={4}>
          {#each filteredProjects as item (item.value)}
            <Combobox.Item value={item.value} label={item.label} class={comboItemClass}>
              {item.label}
            </Combobox.Item>
          {:else}
            <span class="block px-3 py-2 text-sm text-surface-600 dark:text-surface-400">No projects found</span>
          {/each}
        </Combobox.Content>
      </Combobox.Portal>
    </Combobox.Root>
  </div>

  <div
    class="flex items-center gap-4 rounded border border-transparent px-2 py-1.5 focus:border-surface-300 focus:bg-surface-50 dark:focus:border-surface-600 dark:focus:bg-surface-900 outline-none"
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
    <Checkbox id="use-worktree" label="Worktree" bind:checked={useWorktree} tabindex={-1} />
    <span class="text-[10px] text-surface-600 dark:text-surface-400">W</span>
    <Checkbox id="auto-approve" label="Auto-approve" bind:checked={autoApprove} tabindex={-1} />
    <span class="text-[10px] text-surface-600 dark:text-surface-400">A</span>
    {#if providerKeys.length > 1}
      <select
        bind:value={selectedProvider}
        class="ml-auto rounded border border-surface-300 dark:border-surface-600 bg-surface-50 dark:bg-surface-900 px-2 py-1 text-xs text-surface-700 dark:text-surface-300"
        tabindex={-1}
      >
        {#each providerKeys as key (key)}
          <option value={key} selected={key === config.default_provider}>{key}</option>
        {/each}
      </select>
      <span class="text-[10px] text-surface-600 dark:text-surface-400">P</span>
    {/if}
  </div>

  {#if useWorktree}
    <div class="space-y-1">
      <Label>Base branch</Label>
      <Combobox.Root type="single" bind:value={baseBranchValue} onOpenChangeComplete={(o) => { if (!o) baseBranchSearch = baseBranchSearch; }}>
        <Combobox.Input
          oninput={(e) => (baseBranchSearch = e.currentTarget.value)}
          onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
          placeholder="main"
          autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other"
          class={comboInputClass}
        />
        <Combobox.Portal>
          <Combobox.Content class={comboContentClass} sideOffset={4}>
            {#each filteredBaseBranches as item (item.value)}
              <Combobox.Item value={item.value} label={item.label} class={comboItemClass}>
                {item.label}
              </Combobox.Item>
            {:else}
              <span class="block px-3 py-2 text-sm text-surface-600 dark:text-surface-400">No branches found</span>
            {/each}
          </Combobox.Content>
        </Combobox.Portal>
      </Combobox.Root>
    </div>

    <div class="space-y-1">
      <Label>New branch name</Label>
      <Input
        bind:value={newBranchName}
        onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
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
      <Combobox.Root type="single" bind:value={branchValue} onOpenChangeComplete={(o) => { if (!o && !branchValue) branchSearch = branchSearch; }}>
        <Combobox.Input
          oninput={(e) => (branchSearch = e.currentTarget.value)}
          onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
          placeholder="main, feat/new-feature..."
          autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other"
          class={comboInputClass}
        />
        <Combobox.Portal>
          <Combobox.Content class={comboContentClass} sideOffset={4}>
            {#each filteredBranches as item (item.value)}
              <Combobox.Item value={item.value} label={item.label} class={comboItemClass}>
                {item.label}
              </Combobox.Item>
            {:else}
              <span class="block px-3 py-2 text-sm text-surface-600 dark:text-surface-400">No branches found</span>
            {/each}
          </Combobox.Content>
        </Combobox.Portal>
      </Combobox.Root>
    </div>

    {#if isNewBranch && branch}
      <div class="space-y-1">
        <Label>Base branch</Label>
        <Combobox.Root type="single" bind:value={baseBranchValue} onOpenChangeComplete={(o) => { if (!o) baseBranchSearch = baseBranchSearch; }}>
          <Combobox.Input
            oninput={(e) => (baseBranchSearch = e.currentTarget.value)}
            onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
            placeholder="main"
            autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other"
            class={comboInputClass}
          />
          <Combobox.Portal>
            <Combobox.Content class={comboContentClass} sideOffset={4}>
              {#each filteredBaseBranches as item (item.value)}
                <Combobox.Item value={item.value} label={item.label} class={comboItemClass}>
                  {item.label}
                </Combobox.Item>
              {:else}
                <span class="block px-3 py-2 text-sm text-surface-600 dark:text-surface-400">No branches found</span>
              {/each}
            </Combobox.Content>
          </Combobox.Portal>
        </Combobox.Root>
      </div>
      <p class="text-xs text-surface-500">Will create new branch: <span class="font-medium text-surface-900 dark:text-surface-100">{branch}</span> from <span class="font-medium text-surface-900 dark:text-surface-100">{baseBranch}</span></p>
    {/if}
  {/if}

  {#if error}
    <p class="text-xs text-error-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <Button type="button" onclick={onCancel}>Cancel</Button>
    <Button type="submit" variant="primary">Launch</Button>
  </div>
</form>
