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
  class="p-4 bg-neutral-900 border border-neutral-700 rounded-lg w-96"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="text-sm font-semibold text-neutral-200 mb-3">New Session</h2>

  <label class="block mb-2">
    <span class="text-xs text-neutral-400">Project</span>
    <select
      bind:value={selectedProjectId}
      class="mt-1 w-full bg-neutral-800 border border-neutral-600 rounded px-2 py-1 text-sm text-neutral-100 focus:outline-none focus:border-neutral-400"
    >
      {#each projects as project (project.id)}
        <option value={project.id}>{project.name}</option>
      {/each}
    </select>
  </label>

  <label class="block mb-1">
    <span class="text-xs text-neutral-400">Branch</span>
    <input
      type="text"
      bind:value={branch}
      list="branch-list"
      placeholder="main, feat/new-feature..."
      class="mt-1 w-full bg-neutral-800 border border-neutral-600 rounded px-2 py-1 text-sm text-neutral-100 focus:outline-none focus:border-neutral-400"
    />
    <datalist id="branch-list">
      {#each branches as b}
        <option value={b}></option>
      {/each}
    </datalist>
  </label>

  {#if isNewBranch && branch}
    <p class="text-xs text-neutral-500 mb-2">Will create new branch: <span class="text-neutral-300">{branch}</span></p>
  {/if}

  {#if error}
    <p class="text-xs text-red-400 mb-2">{error}</p>
  {/if}

  <div class="flex justify-end gap-2 mt-3">
    <button
      type="button"
      onclick={onCancel}
      class="px-3 py-1 text-xs text-neutral-400 hover:text-neutral-200"
    >
      Cancel
    </button>
    <button
      type="submit"
      class="px-3 py-1 text-xs bg-neutral-700 rounded hover:bg-neutral-600"
    >
      Launch
    </button>
  </div>
</form>
