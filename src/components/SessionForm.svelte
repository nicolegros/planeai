<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Combobox } from "bits-ui";

  interface Project { id: string; name: string; path: string; }
  interface Session { id: string; project_id: string; tmux_name: string; branch: string; status: string; created_at: string; }
  interface Props { projects: Project[]; onCreated: (session: Session) => void; onCancel: () => void; }

  let { projects, onCreated, onCancel }: Props = $props();

  // Project combobox
  let projectValue = $state(projects[0]?.id ?? "");
  let projectSearch = $state("");
  const projectItems = projects.map((p) => ({ value: p.id, label: p.name }));
  const filteredProjects = $derived(
    projectSearch === "" ? projectItems : projectItems.filter((p) => p.label.toLowerCase().includes(projectSearch.toLowerCase())),
  );

  // Branch combobox
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

  const branch = $derived(branchValue || branchSearch);
  const isNewBranch = $derived(branch !== "" && !branches.some((b) => b.value === branch));

  let formEl: HTMLFormElement;
  let error = $state("");

  function focusBranchInput() {
    requestAnimationFrame(() => {
      formEl?.querySelectorAll('input')?.[1]?.focus();
    });
  }

  async function submit() {
    if (!selectedProject || !branch) { error = "Select a project and enter a branch name."; return; }
    try {
      const session = await invoke<Session>("launch_session", {
        projectId: selectedProject.id, projectName: selectedProject.name,
        repoPath: selectedProject.path, branch, isNewBranch,
      });
      onCreated(session);
    } catch (e) { error = String(e); }
  }
</script>

<form bind:this={formEl} class="space-y-4" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <div class="space-y-1">
    <label class="text-sm font-medium">Project</label>
    <Combobox.Root type="single" bind:value={projectValue} onValueChange={focusBranchInput} onOpenChangeComplete={(o) => { if (!o) projectSearch = ""; }}>
      <Combobox.Input
        oninput={(e) => (projectSearch = e.currentTarget.value)}
        onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
        placeholder="Search project..."
        autocomplete="off"
        autocorrect="off"
        autocapitalize="off"
        spellcheck={false}
        data-form-type="other"
        class="w-full rounded border border-gray-300 px-3 py-2 text-sm"
      />
      <Combobox.Portal>
        <Combobox.Content class="z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-gray-200 bg-white shadow-lg" sideOffset={4}>
          {#each filteredProjects as item (item.value)}
            <Combobox.Item value={item.value} label={item.label} class="cursor-pointer px-3 py-2 text-sm data-[highlighted]:bg-gray-100">
              {item.label}
            </Combobox.Item>
          {:else}
            <span class="block px-3 py-2 text-sm text-gray-400">No projects found</span>
          {/each}
        </Combobox.Content>
      </Combobox.Portal>
    </Combobox.Root>
  </div>

  <div class="space-y-1">
    <label class="text-sm font-medium">Branch</label>
    <Combobox.Root type="single" bind:value={branchValue} onOpenChangeComplete={(o) => { if (!o && !branchValue) branchSearch = branchSearch; }}>
      <Combobox.Input
        oninput={(e) => (branchSearch = e.currentTarget.value)}
        onkeydown={(e) => { if (e.key === "Enter" && e.metaKey) { e.preventDefault(); submit(); } }}
        placeholder="main, feat/new-feature..."
        autocomplete="off"
        autocorrect="off"
        autocapitalize="off"
        spellcheck={false}
        data-form-type="other"
        class="w-full rounded border border-gray-300 px-3 py-2 text-sm"
      />
      <Combobox.Portal>
        <Combobox.Content class="z-[100] w-[var(--bits-combobox-anchor-width)] max-h-48 overflow-y-auto rounded border border-gray-200 bg-white shadow-lg" sideOffset={4}>
          {#each filteredBranches as item (item.value)}
            <Combobox.Item value={item.value} label={item.label} class="cursor-pointer px-3 py-2 text-sm data-[highlighted]:bg-gray-100">
              {item.label}
            </Combobox.Item>
          {:else}
            <span class="block px-3 py-2 text-sm text-gray-400">No branches found</span>
          {/each}
        </Combobox.Content>
      </Combobox.Portal>
    </Combobox.Root>
  </div>

  {#if isNewBranch && branch}
    <p class="text-xs text-gray-500">Will create new branch: <span class="font-medium text-gray-900">{branch}</span></p>
  {/if}

  {#if error}
    <p class="text-xs text-red-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <button type="button" onclick={onCancel} class="rounded border border-gray-300 px-3 py-1.5 text-sm">Cancel</button>
    <button type="submit" class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white">Launch</button>
  </div>
</form>
