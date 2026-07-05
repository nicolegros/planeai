<script lang="ts">
  import { onMount } from "svelte";
  import { projects as projectsApi } from "../lib/api";
  import { open } from "@tauri-apps/plugin-dialog";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { nameFromPath } from "../lib/name-from-path";
  import { getSettings } from "../lib/settings.svelte";
  import { Button, Input, Label } from "./ui";
  import { LoaderCircle } from "@lucide/svelte";

  interface Props {
    onCreated: () => void;
    onCancel: () => void;
  }

  let { onCreated, onCancel }: Props = $props();

  const config = $derived(getSettings());
  const basePath = $derived(config.projects_base_path);

  let path = $state(getSettings().projects_base_path ? getSettings().projects_base_path + "/" : "");
  let name = $state("");
  let nameManuallyEdited = $state(false);
  let error = $state("");
  let submitting = $state(false);
  let formEl: HTMLFormElement;

  onMount(() => {
    formEl?.querySelector<HTMLInputElement>("input")?.focus();
  });

  $effect(() => {
    if (!nameManuallyEdited && path) {
      name = nameFromPath(path);
    }
  });

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false, defaultPath: basePath ?? undefined });
    if (selected) {
      path = selected as string;
      name = nameFromPath(path);
      error = "";
    }
  }

  async function submit() {
    if (submitting) return;
    if (!path) {
      error = "Path is required.";
      return;
    }
    if (!name) {
      name = nameFromPath(path);
    }
    if (!name) {
      error = "Could not derive a name from the path.";
      return;
    }
    submitting = true;
    const valid = await projectsApi.validateGitRepo(path);
    if (!valid) {
      error = "Not a valid git repository (no .git found).";
      submitting = false;
      return;
    }
    try {
      await projectsApi.create(name, path);
    } catch (e) {
      showSnackbar(String(e));
      submitting = false;
      return;
    }
    onCreated();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; }
    if (e.key === "Escape") {
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form
  bind:this={formEl}
  class="rounded-lg border border-border bg-panel p-6 w-80 space-y-4 shadow-lg"
  onsubmit={(e) => { e.preventDefault(); submit(); }}
  onkeydown={handleKeydown}
>
  <h2 class="text-lg font-semibold text-t1">Add Project</h2>

  <div class="space-y-1">
    <Label>Repository path</Label>
    <div class="flex gap-2">
      <Input bind:value={path} placeholder="/path/to/repo" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" class="flex-1" />
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
    <Button type="submit" variant="primary" disabled={submitting}>
      {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}Add <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span>{/if}
    </Button>
  </div>
</form>
