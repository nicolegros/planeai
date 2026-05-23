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
  class="rounded-lg border border-gray-200 bg-white p-6 w-80 space-y-4 shadow-lg"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="text-lg font-semibold">Add Project</h2>

  <label class="block space-y-1">
    <span class="text-sm font-medium">Repository path</span>
    <div class="flex gap-2">
      <input type="text" bind:value={path} placeholder="/path/to/repo" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" class="flex-1 rounded border border-gray-300 px-3 py-2 text-sm" />
      <button type="button" onclick={pickFolder} class="rounded border border-gray-300 px-2 py-1 text-sm">Browse</button>
    </div>
  </label>

  <label class="block space-y-1">
    <span class="text-sm font-medium">Name</span>
    <input type="text" bind:value={name} placeholder="my-project" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" class="w-full rounded border border-gray-300 px-3 py-2 text-sm" />
  </label>

  {#if error}
    <p class="text-xs text-red-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <button type="button" onclick={onCancel} class="rounded border border-gray-300 px-3 py-1.5 text-sm">Cancel</button>
    <button type="submit" class="rounded bg-gray-900 px-3 py-1.5 text-sm text-white">Add</button>
  </div>
</form>
