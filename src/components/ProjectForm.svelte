<script lang="ts">
  import { projects as projectsApi } from "../lib/api";
  import { open } from "@tauri-apps/plugin-dialog";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { nameFromPath } from "../lib/name-from-path";
  import { getSettings } from "../lib/settings.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
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
  let submitting = $state(false);
  let submitAttempted = $state(false);
  let wrapperEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!nameManuallyEdited && path) {
      name = nameFromPath(path);
    }
  });

  // Field-level validation
  const fieldErrors = $derived.by(() => {
    const errors: Record<string, string> = {};
    if (!path.trim()) errors.path = "Path is required";
    if (!name.trim()) errors.name = "Name is required";
    return errors;
  });

  const canSubmit = $derived(!submitting && !fieldErrors.path && !fieldErrors.name);

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false, defaultPath: basePath ?? undefined });
    if (selected) {
      path = selected as string;
      name = nameFromPath(path);
    }
  }

  async function submit() {
    submitAttempted = true;
    if (!canSubmit) return;
    submitting = true;

    const valid = await projectsApi.validateGitRepo(path.trim());
    if (!valid) {
      showSnackbar("Not a valid git repository (no .git found).");
      submitting = false;
      return;
    }

    try {
      await projectsApi.create(name.trim(), path.trim());
    } catch (e) {
      showSnackbar(String(e));
      submitting = false;
      return;
    }
    onCreated();
  }

  // Focus wrapper on mount
  $effect(() => { if (wrapperEl) wrapperEl.focus(); });

  // Form keyboard controller
  const fk = createFormKeyboardController(
    () => [
      { key: "p", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='path'] input") ?? null },
      { key: "n", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='name'] input") ?? null },
      { key: "b", toggle: () => { pickFolder(); } },
    ],
    { wrapper: () => wrapperEl, onDismiss: () => onCancel() },
  );

  const badge = $derived(fk.mode === "normal" ? "bg-accent-bg text-accent" : "bg-panel-hi text-t3");

  function metaEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div bind:this={wrapperEl} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none" data-form-keyboard>
<form class="px-5 pb-0 space-y-4" onsubmit={(e) => { e.preventDefault(); submit(); }}>

  <div class="space-y-1" data-field="path">
    <Label>Repository path <span class="font-mono text-[10px] px-1 rounded {badge}">P</span></Label>
    <div class="flex gap-2">
      <Input bind:value={path} onkeydown={metaEnter} placeholder="/path/to/repo" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" class="flex-1" />
      <Button type="button" onclick={pickFolder}>Browse <span class="font-mono text-[10px] px-1 rounded {badge}">B</span></Button>
    </div>
    {#if submitAttempted && fieldErrors.path}
      <p class="text-xs text-red-400 mt-0.5">{fieldErrors.path}</p>
    {/if}
  </div>

  <div class="space-y-1" data-field="name">
    <Label>Name <span class="font-mono text-[10px] px-1 rounded {badge}">N</span></Label>
    <Input bind:value={name} onkeydown={metaEnter} placeholder="my-project" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" oninput={() => { nameManuallyEdited = true; }} />
    {#if submitAttempted && fieldErrors.name}
      <p class="text-xs text-red-400 mt-0.5">{fieldErrors.name}</p>
    {/if}
  </div>

  <div class="flex items-center justify-between pt-2 pb-4 border-t border-border mt-3">
    <div class="flex items-center gap-2">
      {#if fk.mode === "insert"}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-accent-bg text-accent font-medium">INSERT</span>
        <span class="text-[10px] text-t3">esc → normal mode</span>
      {:else}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
        <span class="text-[10px] text-t3">press a key to focus field</span>
      {/if}
    </div>
    <div class="flex gap-2">
      <Button type="button" onclick={() => onCancel()}>Cancel</Button>
      <Button type="submit" variant="primary" disabled={!canSubmit}>
        {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}Add project <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>{/if}
      </Button>
    </div>
  </div>
</form>
</div>
