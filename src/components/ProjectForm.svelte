<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onCreated: () => void;
    onCancel: () => void;
  }

  let { onCreated, onCancel }: Props = $props();

  let path = $state("");
  let name = $state("");
  let error = $state("");

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      path = selected as string;
      // Pre-fill name from directory basename
      const parts = path.replace(/\/$/, "").split("/");
      name = parts[parts.length - 1] || "";
      error = "";
    }
  }

  async function submit() {
    if (!path || !name) {
      error = "Both path and name are required.";
      return;
    }

    const valid = await invoke<boolean>("validate_git_repo", { path });
    if (!valid) {
      error = "Not a valid git repository (no .git found).";
      return;
    }

    await invoke("create_project", { name, path });
    onCreated();
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
  class="p-4 bg-neutral-900 border border-neutral-700 rounded-lg w-80"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="text-sm font-semibold text-neutral-200 mb-3">Add Project</h2>

  <label class="block mb-2">
    <span class="text-xs text-neutral-400">Repository path</span>
    <div class="flex gap-1 mt-1">
      <input
        type="text"
        bind:value={path}
        placeholder="/path/to/repo"
        class="flex-1 bg-neutral-800 border border-neutral-600 rounded px-2 py-1 text-sm text-neutral-100 focus:outline-none focus:border-neutral-400"
      />
      <button
        type="button"
        onclick={pickFolder}
        class="px-2 py-1 bg-neutral-700 rounded text-xs hover:bg-neutral-600"
      >
        Browse
      </button>
    </div>
  </label>

  <label class="block mb-3">
    <span class="text-xs text-neutral-400">Name</span>
    <input
      type="text"
      bind:value={name}
      placeholder="my-project"
      class="mt-1 w-full bg-neutral-800 border border-neutral-600 rounded px-2 py-1 text-sm text-neutral-100 focus:outline-none focus:border-neutral-400"
    />
  </label>

  {#if error}
    <p class="text-xs text-red-400 mb-2">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
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
      Add
    </button>
  </div>
</form>
