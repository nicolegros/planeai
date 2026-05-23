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
  class="card preset-outlined-surface-200-800 p-6 w-80 space-y-4"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="h5">Add Project</h2>

  <label class="label">
    <span class="label-text">Repository path</span>
    <div class="flex gap-2 mt-1">
      <input type="text" bind:value={path} placeholder="/path/to/repo" class="input flex-1" />
      <button type="button" onclick={pickFolder} class="btn btn-sm preset-tonal-surface">Browse</button>
    </div>
  </label>

  <label class="label">
    <span class="label-text">Name</span>
    <input type="text" bind:value={name} placeholder="my-project" class="input mt-1" />
  </label>

  {#if error}
    <p class="text-xs text-error-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <button type="button" onclick={onCancel} class="btn btn-sm preset-tonal-surface">Cancel</button>
    <button type="submit" class="btn btn-sm preset-filled-primary-500">Add</button>
  </div>
</form>
