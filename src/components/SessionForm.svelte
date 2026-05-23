<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface Project {
    id: string;
    name: string;
    path: string;
  }

  interface Session {
    id: string;
    project_id: string;
    tmux_name: string;
    branch: string;
    status: string;
    created_at: string;
  }

  interface Props {
    projects: Project[];
    onCreated: (session: Session) => void;
    onCancel: () => void;
  }

  let { projects, onCreated, onCancel }: Props = $props();

  let selectedProjectId = $state(projects[0]?.id ?? "");
  let branch = $state("");
  let branches = $state<string[]>([]);
  let error = $state("");

  const selectedProject = $derived(projects.find((p) => p.id === selectedProjectId));

  $effect(() => {
    if (selectedProject) {
      invoke<string[]>("list_branches", { repoPath: selectedProject.path }).then(
        (b) => (branches = b),
        () => (branches = []),
      );
    }
  });

  const isNewBranch = $derived(branch !== "" && !branches.includes(branch));

  async function submit() {
    if (!selectedProject || !branch) {
      error = "Select a project and enter a branch name.";
      return;
    }
    try {
      const session = await invoke<Session>("launch_session", {
        projectId: selectedProject.id,
        projectName: selectedProject.name,
        repoPath: selectedProject.path,
        branch,
        isNewBranch,
      });
      onCreated(session);
    } catch (e) {
      error = String(e);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form
  class="card preset-outlined-surface-200-800 p-6 w-96 space-y-4"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="h5">New Session</h2>

  <label class="label">
    <span class="label-text">Project</span>
    <select bind:value={selectedProjectId} class="select mt-1">
      {#each projects as project (project.id)}
        <option value={project.id}>{project.name}</option>
      {/each}
    </select>
  </label>

  <label class="label">
    <span class="label-text">Branch</span>
    <input
      type="text"
      bind:value={branch}
      list="branch-list"
      placeholder="main, feat/new-feature..."
      class="input mt-1"
    />
    <datalist id="branch-list">
      {#each branches as b}
        <option value={b}></option>
      {/each}
    </datalist>
  </label>

  {#if isNewBranch && branch}
    <p class="text-xs text-surface-400">Will create new branch: <span class="text-surface-100">{branch}</span></p>
  {/if}

  {#if error}
    <p class="text-xs text-error-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <button type="button" onclick={onCancel} class="btn btn-sm preset-tonal-surface">Cancel</button>
    <button type="submit" class="btn btn-sm preset-filled-primary-500">Launch</button>
  </div>
</form>
