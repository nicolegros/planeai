<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { Button, Input, Label } from "./ui";

  interface Props {
    onCreated: () => void;
    onCancel: () => void;
  }

  let { onCreated, onCancel }: Props = $props();

  let path = $state("");
  let name = $state("");
  let nameManuallyEdited = $state(false);
  let error = $state("");

  $effect(() => {
    if (!nameManuallyEdited && path) {
      name = path.replace(/\/$/, "").split("/").pop() || "";
    }
  });

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
    if (!path) {
      error = "Path is required.";
      return;
    }
    if (!name) {
      name = path.replace(/\/$/, "").split("/").pop() || "";
    }
    if (!name) {
      error = "Could not derive a name from the path.";
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
  class="rounded-lg border border-surface-200 bg-surface-50 p-6 w-80 space-y-4 shadow-lg dark:border-surface-700 dark:bg-surface-900"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="text-lg font-semibold text-surface-900 dark:text-surface-50">Add Project</h2>

  <div class="space-y-1">
    <Label>Repository path</Label>
    <div class="flex gap-2">
      <Input bind:value={path} placeholder="/path/to/repo" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" class="flex-1" autofocus />
      <Button type="button" onclick={pickFolder}>Browse</Button>
    </div>
  </div>

  <div class="space-y-1">
    <Label>Name</Label>
    <Input bind:value={name} placeholder="my-project" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" oninput={() => { nameManuallyEdited = true; }} />
  </div>

  {#if error}
    <p class="text-xs text-error-500">{error}</p>
  {/if}

  <div class="flex justify-end gap-2">
    <Button type="button" onclick={onCancel}>Cancel</Button>
    <Button type="submit" variant="primary">Add</Button>
  </div>
</form>
