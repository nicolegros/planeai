<script lang="ts">
  import { ArrowLeft, Check, CircleAlert, Folder } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import appIcon from "../assets/app-icon.png";
  import { config as configApi, preferences } from "../lib/api";
  import { setKeyboardSuspended } from "../lib/keyboard";
  import { initialAgentSelection, onboardingAgentsPatch } from "../lib/onboarding";
  import { PROVIDER_PRESETS } from "../lib/provider-presets";
  import { saveSettings } from "../lib/save-settings";
  import { getSettings, type AppearanceMode } from "../lib/settings.svelte";
  import { Button, SegmentedControl, Select } from "./ui";

  const STEPS = [
    { title: "Which agents do you use?", subtitle: "We checked your PATH. You can add custom agents later in Settings." },
    { title: "Where do your projects live?", subtitle: "PlaneAI suggests this folder when you add or clone a project." },
    { title: "Pick a look", subtitle: "Applied right away. You can change it any time in Settings." },
  ] as const;

  const config = $derived(getSettings());
  let step = $state(0);
  let saving = $state(false);
  let detected = $state<Record<string, string | null> | null>(null);
  let selected = $state<string[]>([]);
  let defaultKey = $state<string | null>(null);
  // svelte-ignore state_referenced_locally
  let projectsFolder = $state(config.projects_base_path ?? "");
  let themes = $state<string[]>([]);
  const nothingFound = $derived(detected !== null && PROVIDER_PRESETS.every((preset) => !detected![preset.key]));

  // App shortcuts would act on the hidden workspace behind the wizard.
  $effect(() => {
    setKeyboardSuspended(true);
    return () => setKeyboardSuspended(false);
  });

  $effect(() => {
    void detect(true);
    preferences
      .listThemes()
      .then((names) => (themes = names))
      .catch((error) => console.warn("Failed to list themes:", error));
  });

  /** Re-detects agents; the first run also seeds the selection, later runs add newly found agents. */
  async function detect(seed: boolean) {
    try {
      const result = await configApi.detectProviders();
      const initial = initialAgentSelection(config, result);
      if (seed) {
        selected = initial.selected;
        defaultKey = initial.defaultKey;
      } else {
        selected = PROVIDER_PRESETS.map((preset) => preset.key).filter((key) => selected.includes(key) || (result[key] && !detected?.[key]));
        defaultKey ??= selected[0] ?? null;
      }
      detected = result;
    } catch (error) {
      console.warn("Agent detection failed:", error);
      detected ??= {};
    }
  }

  function toggle(key: string) {
    selected = selected.includes(key) ? selected.filter((candidate) => candidate !== key) : PROVIDER_PRESETS.map((preset) => preset.key).filter((candidate) => candidate === key || selected.includes(candidate));
    if (!defaultKey || !selected.includes(defaultKey)) defaultKey = selected[0] ?? null;
  }

  async function addSearchPath() {
    try {
      const folder = await open({ directory: true, multiple: false });
      if (typeof folder !== "string") return;
      const dirs = config.extra_path_dirs ?? [];
      if (!dirs.includes(folder) && !(await saveSettings({ extra_path_dirs: [...dirs, folder] }))) return;
      await detect(false);
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
        const patch = onboardingAgentsPatch(config, selected, defaultKey);
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

<div class="fixed inset-0 z-[95] flex flex-col bg-canvas text-t1" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
  <!-- The window uses an overlay titlebar, so the top strip keeps it draggable. -->
  <div data-tauri-drag-region class="h-10 shrink-0"></div>
  <div class="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-4 pb-10">
    <div class="w-full max-w-[480px]">
      <header class="mb-6 text-center">
        <img src={appIcon} alt="" class="mx-auto mb-4 h-14 w-14" />
        <h1 id="onboarding-title" class="text-[20px] font-semibold tracking-tight">{STEPS[step].title}</h1>
        <p class="mt-1 text-[13px] text-t3">{STEPS[step].subtitle}</p>
      </header>

      <div class="rounded-xl border border-border-s bg-panel p-1.5 shadow-sm">
        {#if step === 0}
          {#if detected === null}
            <p class="px-3 py-6 text-center text-[13px] text-t3">Looking for agents…</p>
          {:else}
            <div role="group" aria-label="Agents">
              {#each PROVIDER_PRESETS as preset (preset.key)}
                {@const isSelected = selected.includes(preset.key)}
                {@const path = detected[preset.key]}
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={isSelected}
                  class="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-panel-hi/60"
                  onclick={() => toggle(preset.key)}
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
            {:else if selected.length > 1}
              <div class="mx-3 mb-1.5 mt-1 flex items-center justify-between gap-3 border-t border-border pt-3 text-[13px]">
                <span class="text-t2">Default agent</span>
                <div class="w-44">
                  <Select
                    items={PROVIDER_PRESETS.filter((preset) => selected.includes(preset.key)).map((preset) => ({ value: preset.key, label: preset.label }))}
                    value={defaultKey ?? ""}
                    onValueChange={(key) => (defaultKey = key)}
                    ariaLabel="Default agent"
                  />
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
            <SegmentedControl
              name="onboarding-appearance-mode"
              label="Mode"
              options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]}
              value={config.appearance.mode}
              onValueChange={(mode: AppearanceMode) => saveSettings({ appearance: { ...config.appearance, mode } })}
            />
            <div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Theme">
              {#each themes.includes(config.appearance.theme) ? themes : [config.appearance.theme, ...themes] as theme (theme)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={config.appearance.theme === theme}
                  class="rounded-md px-3 py-1 text-[12px] font-medium capitalize transition-colors {config.appearance.theme === theme ? 'bg-accent text-on-accent' : 'bg-panel-hi text-t2 hover:text-t1'}"
                  onclick={() => saveSettings({ appearance: { ...config.appearance, theme } })}
                >{theme}</button>
              {/each}
            </div>
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
        <div class="flex gap-1.5" aria-label="Step {step + 1} of {STEPS.length}" role="img">
          {#each STEPS as entry, index (entry.title)}
            <span class="h-1.5 rounded-full transition-all {index === step ? 'w-4 bg-accent' : 'w-1.5 bg-border-s'}"></span>
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
