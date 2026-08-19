<script lang="ts">
  import { projects as projectsApi, git } from "../lib/api";
  import type { Project } from "../lib/types";
  import * as projectStore from "../lib/project-store.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { nameFromPath } from "../lib/name-from-path";
  import { getSettings } from "../lib/settings.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { Button, Input, Label } from "./ui";
  import { LoaderCircle } from "@lucide/svelte";

  interface Props {
    project?: Project | null;
    onCreated: () => void;
    onCancel: () => void;
  }

  let { project = null, onCreated, onCancel }: Props = $props();
  const isEditing = $derived(project !== null);

  const config = $derived(getSettings());
  const basePath = $derived(config.projects_base_path);

  type FormMode = "local" | "remote";
  let mode = $state<FormMode>("local");

  // Local mode fields
  let path = $state(project?.path ?? (getSettings().projects_base_path ? getSettings().projects_base_path + "/" : ""));
  let name = $state(project?.name ?? "");
  let nameManuallyEdited = $state(project !== null);

  // Remote mode fields
  let cloneUrl = $state("");
  let destination = $state(getSettings().projects_base_path ?? "");
  let remoteName = $state("");
  let remoteNameManuallyEdited = $state(false);

  let submitting = $state(false);
  let submitAttempted = $state(false);
  let wrapperEl = $state<HTMLDivElement | null>(null);

  // Auto-derive name from path (local mode)
  $effect(() => {
    if (mode === "local" && !nameManuallyEdited && path) {
      name = nameFromPath(path);
    }
  });

  // Auto-derive name from clone URL (remote mode)
  $effect(() => {
    if (mode === "remote" && !remoteNameManuallyEdited && cloneUrl) {
      remoteName = repoNameFromUrl(cloneUrl);
    }
  });

  function repoNameFromUrl(url: string): string {
    // Handle both SSH (git@host:user/repo.git) and HTTPS (https://host/user/repo.git)
    const stripped = url.replace(/\.git\/?$/, "").replace(/\/$/, "");
    const lastSegment = stripped.split(/[/:]/).pop() || "";
    return lastSegment;
  }

  // Field-level validation
  const fieldErrors = $derived.by(() => {
    const errors: Record<string, string> = {};
    if (mode === "local") {
      if (!path.trim()) errors.path = "Path is required";
      if (!name.trim()) errors.name = "Name is required";
    } else {
      if (!cloneUrl.trim()) errors.url = "Clone URL is required";
      if (!destination.trim()) errors.destination = "Destination is required";
      if (!remoteName.trim()) errors.name = "Name is required";
    }
    return errors;
  });

  const canSubmit = $derived(!submitting && Object.keys(fieldErrors).length === 0);

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false, defaultPath: basePath ?? undefined });
    if (selected) {
      if (mode === "local") {
        path = selected as string;
        name = nameFromPath(path);
      } else {
        destination = selected as string;
      }
    }
  }

  async function submit() {
    submitAttempted = true;
    if (!canSubmit) return;
    submitting = true;

    if (mode === "local") {
      await submitLocal();
    } else {
      await submitRemote();
    }
  }

  async function submitLocal() {
    const valid = await projectsApi.validateGitRepo(path.trim());
    if (!valid) {
      showSnackbar("Not a valid git repository (no .git found).");
      submitting = false;
      return;
    }

    try {
      if (project) {
        await projectStore.updateProject(project.id, name.trim(), path.trim());
      } else {
        await projectStore.createProject(name.trim(), path.trim());
      }
    } catch (e) {
      showSnackbar(String(e));
      submitting = false;
      return;
    }
    onCreated();
  }

  async function submitRemote() {
    const dest = destination.trim().replace(/\/$/, "");
    const repoName = remoteName.trim();
    if (!repoName) {
      showSnackbar("Could not determine repository name from URL.");
      submitting = false;
      return;
    }
    const fullPath = `${dest}/${repoName}`;

    try {
      await git.cloneRepository(cloneUrl.trim(), fullPath);
    } catch (e) {
      showSnackbar(String(e));
      submitting = false;
      return;
    }

    try {
      await projectStore.createProject(repoName, fullPath);
    } catch (e) {
      showSnackbar(`Clone succeeded but project creation failed: ${String(e)}. The cloned directory remains at ${fullPath}.`);
      submitting = false;
      return;
    }
    onCreated();
  }

  function toggleMode() {
    mode = mode === "local" ? "remote" : "local";
    submitAttempted = false;
  }

  // Focus wrapper on mount
  $effect(() => { if (wrapperEl) wrapperEl.focus(); });

  // Form keyboard controller
  const fk = createFormKeyboardController(
    () => isEditing
      ? [
          { key: "p", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='path'] input") ?? null },
          { key: "n", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='name'] input") ?? null },
          { key: "b", toggle: () => { pickFolder(); } },
        ]
      : mode === "local"
        ? [
            { key: "t", toggle: () => toggleMode() },
            { key: "p", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='path'] input") ?? null },
            { key: "n", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='name'] input") ?? null },
            { key: "b", toggle: () => { pickFolder(); } },
          ]
        : [
            { key: "t", toggle: () => toggleMode() },
            { key: "u", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='url'] input") ?? null },
            { key: "d", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='destination'] input") ?? null },
            { key: "b", toggle: () => { pickFolder(); } },
            { key: "n", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='name'] input") ?? null },
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

  {#if !isEditing}
  <!-- Mode toggle -->
  <div class="flex gap-1 rounded-md bg-panel-hi p-0.5" role="tablist" aria-label="Project source">
    <button type="button" role="tab" aria-selected={mode === "local"} aria-controls="panel-local" class="flex-1 text-xs py-1 px-2 rounded transition-colors {mode === 'local' ? 'bg-accent text-on-accent' : 'bg-panel border border-border text-t2 hover:bg-panel-hi'}" onclick={() => { mode = "local"; submitAttempted = false; }}>
      Local {#if mode !== "local"}<span class="font-mono text-[10px] px-1 rounded {badge}">T</span>{/if}
    </button>
    <button type="button" role="tab" aria-selected={mode === "remote"} aria-controls="panel-remote" class="flex-1 text-xs py-1 px-2 rounded transition-colors {mode === 'remote' ? 'bg-accent text-on-accent' : 'bg-panel border border-border text-t2 hover:bg-panel-hi'}" onclick={() => { mode = "remote"; submitAttempted = false; }}>
      Git remote {#if mode !== "remote"}<span class="font-mono text-[10px] px-1 rounded {badge}">T</span>{/if}
    </button>
  </div>
  {/if}

  {#if mode === "local"}
    <!-- Local mode: path + name (same as before) -->
    <div id="panel-local" role="tabpanel" class="space-y-4">
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
    </div>
  {:else}
    <!-- Remote mode: url + destination + name -->
    <div id="panel-remote" role="tabpanel" class="space-y-4">
    <div class="space-y-1" data-field="url">
      <Label>Clone URL <span class="font-mono text-[10px] px-1 rounded {badge}">U</span></Label>
      <Input bind:value={cloneUrl} onkeydown={metaEnter} placeholder="git@github.com:user/repo.git" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" />
      {#if submitAttempted && fieldErrors.url}
        <p class="text-xs text-red-400 mt-0.5">{fieldErrors.url}</p>
      {/if}
    </div>

    <div class="space-y-1" data-field="destination">
      <Label>Destination <span class="font-mono text-[10px] px-1 rounded {badge}">D</span></Label>
      <div class="flex gap-2">
        <Input bind:value={destination} onkeydown={metaEnter} placeholder="/path/to/directory" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" class="flex-1" />
        <Button type="button" onclick={pickFolder}>Browse <span class="font-mono text-[10px] px-1 rounded {badge}">B</span></Button>
      </div>
      {#if submitAttempted && fieldErrors.destination}
        <p class="text-xs text-red-400 mt-0.5">{fieldErrors.destination}</p>
      {/if}
    </div>

    <div class="space-y-1" data-field="name">
      <Label>Name <span class="font-mono text-[10px] px-1 rounded {badge}">N</span></Label>
      <Input bind:value={remoteName} onkeydown={metaEnter} placeholder="my-project" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck={false} data-form-type="other" oninput={() => { remoteNameManuallyEdited = true; }} />
      {#if submitAttempted && fieldErrors.name}
        <p class="text-xs text-red-400 mt-0.5">{fieldErrors.name}</p>
      {/if}
    </div>
    </div>
  {/if}

  <div class="sticky bottom-0 bg-panel flex items-center justify-between pt-2 pb-4 border-t border-border mt-3">
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
        {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}{isEditing ? "Save project" : mode === "local" ? "Add project" : "Clone & add"} <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>{/if}
      </Button>
    </div>
  </div>
</form>
</div>
