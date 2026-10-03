<script lang="ts">
  import { tick } from "svelte";
  import { ArrowLeft, Check, CircleAlert, Folder } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import appIcon from "../assets/app-icon.png";
  import { config as configApi } from "../lib/api";
  import { isInDialog } from "../lib/dialog-focus";
  import {
    defaultAgentOptions,
    initialAgentSelection,
    mergeDetection,
    onboardingAgentsPatch,
    toggleAgent,
    type AgentSelection,
  } from "../lib/onboarding";
  import { PROVIDER_PRESETS } from "../lib/provider-presets";
  import { saveSettings } from "../lib/save-settings";
  import { getSettings } from "../lib/settings.svelte";
  import ModePicker from "./settings/ModePicker.svelte";
  import ThemePicker from "./settings/ThemePicker.svelte";
  import { Button, Select } from "./ui";

  const STEPS = [
    { title: "Which agents do you use?", subtitle: "We checked your PATH. You can add custom agents later in Settings." },
    { title: "Where do your projects live?", subtitle: "PlaneAI suggests this folder when you add or clone a project." },
    { title: "Pick a look", subtitle: "Applied right away. You can change it any time in Settings." },
  ] as const;

  const config = $derived(getSettings());
  let step = $state(0);
  let saving = $state(false);
  let heading = $state<HTMLHeadingElement | null>(null);
  let detected = $state<Record<string, string | null> | null>(null);
  let selection = $state<AgentSelection>({ selected: [], defaultKey: null });
  // svelte-ignore state_referenced_locally
  let projectsFolder = $state(config.projects_base_path ?? "");
  const nothingFound = $derived(detected !== null && PROVIDER_PRESETS.every((preset) => !detected![preset.key]));
  const defaultOptions = $derived(defaultAgentOptions(config, selection.selected));

  // Each step announces itself and takes focus, so nothing behind the wizard keeps it.
  $effect(() => {
    void step;
    void tick().then(() => heading?.focus());
  });

  $effect(() => {
    void detect();
  });

  // Keys from outside a dialog (<body> or the inert workspace) stop here, before bubble-phase
  // workspace listeners; earlier capture listeners guard themselves against dialogs.
  $effect(() => {
    const block = (event: KeyboardEvent) => {
      if (isInDialog(event.target)) return;
      event.stopImmediatePropagation();
      // Focus falls to <body> when a prompt over the wizard closes; bring it back for the next key.
      if (event.target === document.body) heading?.focus();
    };
    window.addEventListener("keydown", block, true);
    return () => window.removeEventListener("keydown", block, true);
  });

  async function detect() {
    try {
      const result = await configApi.detectProviders();
      selection = detected === null ? initialAgentSelection(config, result) : mergeDetection(selection, detected, result);
      detected = result;
    } catch (error) {
      console.warn("Agent detection failed:", error);
      detected ??= {};
    }
  }

  async function addSearchPath() {
    try {
      const folder = await open({ directory: true, multiple: false });
      if (typeof folder !== "string") return;
      const dirs = config.extra_path_dirs ?? [];
      if (!dirs.includes(folder) && !(await saveSettings({ extra_path_dirs: [...dirs, folder] }))) return;
      await detect();
    } catch (error) {
      console.warn("Failed to add a search path:", error);
    }
  }

  async function chooseProjectsFolder() {
    try {
      const folder = await open({ directory: true, multiple: false, defaultPath: projectsFolder || undefined });
      if (typeof folder === "string") projectsFolder = folder;
    } catch (error) {
      console.warn("Failed to choose the projects folder:", error);
    }
  }

  /** Saves the current step, then advances; the last step finishes setup. */
  async function next() {
    saving = true;
    try {
      if (step === 0) {
        const patch = onboardingAgentsPatch(config, selection.selected, selection.defaultKey);
        if (patch && !(await saveSettings(patch))) return;
      } else if (step === 1) {
        const folder = projectsFolder.trim() || null;
        if (folder !== (config.projects_base_path ?? null) && !(await saveSettings({ projects_base_path: folder }))) return;
      }
      if (step < STEPS.length - 1) step += 1;
      else await saveSettings({ onboarding_completed: true });
    } finally {
      saving = false;
    }
  }

  function skip() {
    void saveSettings({ onboarding_completed: true });
  }
</script>

<!-- Below dialogs (z-50) so a quit confirmation still shows; the workspace behind is inert. -->
<div class="fixed inset-0 z-40 flex flex-col bg-canvas text-t1 focus:outline-none" role="dialog" aria-modal="true" aria-labelledby="onboarding-title" tabindex="-1">
  <!-- The window uses an overlay titlebar, so the top strip keeps it draggable. -->
  <div data-tauri-drag-region class="h-10 shrink-0"></div>
  <div class="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-4 pb-10">
    <div class="w-full max-w-[480px]">
      <header class="mb-6 text-center">
        <img src={appIcon} alt="" class="mx-auto mb-4 h-14 w-14" />
        <h1 bind:this={heading} id="onboarding-title" tabindex="-1" aria-describedby="onboarding-step" class="text-[20px] font-semibold tracking-tight focus:outline-none">{STEPS[step].title}</h1>
        <p id="onboarding-step" class="sr-only">Step {step + 1} of {STEPS.length}</p>
        <p class="mt-1 text-[13px] text-t3">{STEPS[step].subtitle}</p>
      </header>

      <div class="rounded-xl border border-border-s bg-panel p-1.5 shadow-sm">
        {#if step === 0}
          {#if detected === null}
            <p class="px-3 py-6 text-center text-[13px] text-t3">Looking for agents…</p>
          {:else}
            <div role="group" aria-label="Agents">
              {#each PROVIDER_PRESETS as preset (preset.key)}
                {@const isSelected = selection.selected.includes(preset.key)}
                {@const path = detected[preset.key]}
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={isSelected}
                  class="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-panel-hi/60"
                  onclick={() => (selection = toggleAgent(selection, preset.key))}
                >
                  <span class="grid h-4 w-4 shrink-0 place-items-center rounded border {isSelected ? 'border-accent bg-accent text-on-accent' : 'border-border-s'}">
                    {#if isSelected}<Check size={11} strokeWidth={3} />{/if}
                  </span>
                  <span class="flex-1 text-[13px]">{preset.label}</span>
                  <span class="truncate text-[11px] {path ? 'text-status-running' : 'text-t3'}" title={path ?? undefined}>{path ? "Found on PATH" : "Not installed"}</span>
                </button>
              {/each}
            </div>
            {#if nothingFound}
              <div class="mx-3 mb-1.5 mt-1 flex items-center justify-between gap-3 border-t border-border pt-3 text-[12px]">
                <span class="flex items-center gap-1.5 text-t2"><CircleAlert size={12} class="text-status-review" />No agents found on your PATH.</span>
                <Button type="button" size="sm" onclick={addSearchPath}>Add a folder to PATH…</Button>
              </div>
            {:else if defaultOptions.length > 1}
              <div class="mx-3 mb-1.5 mt-1 flex items-center justify-between gap-3 border-t border-border pt-3 text-[13px]">
                <span class="text-t2">Default agent</span>
                <div class="w-44">
                  <Select items={defaultOptions} value={selection.defaultKey ?? ""} onValueChange={(key) => (selection.defaultKey = key)} ariaLabel="Default agent" />
                </div>
              </div>
            {/if}
          {/if}
        {:else if step === 1}
          <div class="flex items-center gap-2 p-2">
            <Folder size={16} class="ml-1 shrink-0 text-t3" />
            <input
              bind:value={projectsFolder}
              placeholder="e.g. ~/Developer"
              aria-label="Projects folder"
              autocomplete="off"
              spellcheck={false}
              class="h-8 min-w-0 flex-1 rounded-md bg-transparent px-1 font-mono text-[13px] text-t1 placeholder:text-t3 focus:outline-none"
            />
            <Button type="button" onclick={chooseProjectsFolder}>Choose…</Button>
          </div>
        {:else}
          <div class="space-y-4 p-3">
            <ModePicker name="onboarding-appearance-mode" />
            <ThemePicker />
          </div>
        {/if}
      </div>

      <footer class="mt-6 flex items-center justify-between">
        <div class="w-24">
          {#if step === 0}
            <button type="button" class="text-[12px] text-t3 hover:text-t1" onclick={skip}>Skip setup</button>
          {:else}
            <button type="button" class="inline-flex items-center gap-1 text-[12px] text-t2 hover:text-t1" onclick={() => (step -= 1)}><ArrowLeft size={12} />Back</button>
          {/if}
        </div>
        <div class="flex gap-1.5">
          {#each STEPS as entry, index (entry.title)}
            <span aria-hidden="true" class="h-1.5 rounded-full transition-all {index === step ? 'w-4 bg-accent' : 'w-1.5 bg-border-s'}"></span>
          {/each}
        </div>
        <div class="flex w-24 justify-end">
          <Button type="button" variant="primary" disabled={saving || (step === 0 && detected === null)} onclick={next}>
            {step < STEPS.length - 1 ? "Continue" : "Finish"}
          </Button>
        </div>
      </footer>
    </div>
  </div>
</div>
