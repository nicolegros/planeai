<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getSettings, updateSettings, type AppearanceMode, type AppConfig, type Provider, type TaskManager } from "../lib/settings.svelte";
  import { getThemesByVariant } from "../lib/terminal-themes";
  import { Select, Input } from "./ui";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();

  const config = $derived(getSettings());
  const darkThemes = getThemesByVariant("dark");
  const lightThemes = getThemesByVariant("light");

  let fontItems = $state<{ value: string; label: string }[]>([]);
  let availableThemes = $state<string[]>([]);
  let editingProvider = $state<string | null>(null);
  let newProviderName = $state("");
  let newProviderCommand = $state("");
  let newProviderYoloFlag = $state("");
  let showAddProvider = $state(false);
  let tmuxAvailable = $state(true);

  const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  onMount(async () => {
    const fonts = await invoke<string[]>("list_monospace_fonts");
    fontItems = fonts.map((f) => ({ value: f, label: f }));
    availableThemes = await invoke<string[]>("list_themes");
    tmuxAvailable = await invoke<boolean>("check_tmux_available");
  });

  const backendValue = $derived(config.session_backend ?? "auto");

  function setSessionBackend(value: string) {
    const backend = value === "auto" ? null : value;
    updateSettings({ session_backend: backend } as Partial<AppConfig>);
  }

  function setAppearance(mode: AppearanceMode) {
    updateSettings({ appearance: { ...config.appearance, mode } });
  }

  function setFontSize(size: number) {
    if (size >= 8 && size <= 32) updateSettings({ terminal: { ...config.terminal, font_size: size } });
  }

  function setDefaultProvider(key: string) {
    updateSettings({ default_provider: key } as Partial<AppConfig>);
  }

  function addProvider() {
    if (!newProviderName || !newProviderCommand) return;
    const providers = { ...config.providers };
    providers[newProviderName] = {
      command: newProviderCommand,
      yolo_flag: newProviderYoloFlag || null,
    };
    updateSettings({ providers } as Partial<AppConfig>);
    newProviderName = "";
    newProviderCommand = "";
    newProviderYoloFlag = "";
    showAddProvider = false;
  }

  function removeProvider(key: string) {
    const providers = { ...config.providers };
    delete providers[key];
    const patch: Partial<AppConfig> = { providers };
    if (config.default_provider === key) {
      patch.default_provider = Object.keys(providers)[0] || "";
    }
    updateSettings(patch);
  }

  function updateProvider(key: string, field: keyof Provider, value: string) {
    const providers = { ...config.providers };
    providers[key] = { ...providers[key], [field]: value || null };
    updateSettings({ providers } as Partial<AppConfig>);
  }

  // Task manager state
  let showAddTaskManager = $state(false);
  let newTmName = $state("");
  let newTmGetTask = $state("kanban show {key}");
  let newTmMoveTask = $state("kanban move {key} {status}");
  let newTmListTasks = $state("kanban list --status todo");

  const taskManagers = $derived(config.task_managers ?? {});

  function addTaskManager() {
    if (!newTmName || !newTmGetTask || !newTmMoveTask || !newTmListTasks) return;
    const tms = { ...taskManagers };
    tms[newTmName] = {
      get_task: newTmGetTask, move_task: newTmMoveTask, list_tasks: newTmListTasks,
      templates: { branch: "{key:lower}/{title:slug}", name: "{key:upper}: {title}", prompt: "Implement task {key}: {title}\n\n{description}" },
      on_start: { move_to: "in_progress" },
      on_notify: { move_to: "in_review" },
      on_restart: { move_to: "in_progress" },
      on_complete: { move_to: "done" },
    };
    const patch: Partial<AppConfig> = { task_managers: tms };
    if (!config.default_task_manager) patch.default_task_manager = newTmName;
    updateSettings(patch);
    newTmName = ""; newTmGetTask = "kanban show {key}"; newTmMoveTask = "kanban move {key} {status}"; newTmListTasks = "kanban list --status todo";
    showAddTaskManager = false;
  }

  function removeTaskManager(key: string) {
    const tms = { ...taskManagers };
    delete tms[key];
    const patch: Partial<AppConfig> = { task_managers: tms };
    if (config.default_task_manager === key) {
      patch.default_task_manager = Object.keys(tms)[0] || null;
    }
    updateSettings(patch);
  }

  function updateTaskManager(key: string, field: string, value: string) {
    const tms = { ...taskManagers };
    tms[key] = { ...tms[key], [field]: value || undefined };
    updateSettings({ task_managers: tms } as Partial<AppConfig>);
  }

  function updateTmTemplate(key: string, field: string, value: string) {
    const tms = { ...taskManagers };
    const templates = { ...(tms[key].templates ?? {}) };
    (templates as any)[field] = value || null;
    tms[key] = { ...tms[key], templates };
    updateSettings({ task_managers: tms } as Partial<AppConfig>);
  }

  function updateTmHook(key: string, hookName: string, value: string) {
    const tms = { ...taskManagers };
    (tms[key] as any)[hookName] = value ? { move_to: value } : null;
    updateSettings({ task_managers: tms } as Partial<AppConfig>);
  }

  function setDefaultTaskManager(key: string) {
    updateSettings({ default_task_manager: key } as Partial<AppConfig>);
  }
</script>

<div class="h-full overflow-y-auto bg-surface-50 dark:bg-surface-950 p-8">
  <div class="max-w-2xl mx-auto space-y-8">
    <div class="flex items-center gap-3">
      <button
        class="rounded p-1.5 text-surface-700 hover:text-surface-900 dark:text-surface-300 dark:hover:text-surface-100 hover:bg-surface-200 dark:hover:bg-surface-800"
        onclick={onBack}
        aria-label="Back"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>
      </button>
      <h1 class="text-xl font-semibold text-surface-900 dark:text-surface-50">Preferences</h1>
    </div>

    <!-- Appearance Mode -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Appearance</h2>
      <div class="flex gap-2">
        {#each ["system", "light", "dark"] as mode (mode)}
          <button
            class="px-4 py-2 rounded-md text-sm font-medium capitalize transition-colors {config.appearance.mode === mode ? 'bg-primary-500 text-white' : 'bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700'}"
            onclick={() => setAppearance(mode as AppearanceMode)}
          >{mode}</button>
        {/each}
      </div>
    </section>

    <!-- Theme -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Theme</h2>
      <div class="flex flex-wrap gap-2">
        {#each availableThemes as themeName (themeName)}
          <button
            class="px-4 py-2 rounded-md text-sm font-medium capitalize transition-colors {config.appearance.theme === themeName ? 'bg-primary-500 text-white' : 'bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700'}"
            onclick={() => updateSettings({ appearance: { ...config.appearance, theme: themeName } })}
          >{themeName}</button>
        {/each}
      </div>
    </section>

    <!-- Dark Terminal Theme -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Dark Terminal Theme</h2>
      <div class="grid grid-cols-3 gap-3">
        {#each darkThemes as theme (theme.id)}
          <button
            class="rounded-lg border-2 p-3 text-left transition-colors {config.appearance.terminal_theme_dark === theme.id ? 'border-primary-500' : 'border-surface-200 dark:border-surface-700 hover:border-surface-400 dark:hover:border-surface-500'}"
            onclick={() => updateSettings({ appearance: { ...config.appearance, terminal_theme_dark: theme.id } })}
          >
            <div class="rounded h-16 mb-2 flex items-end p-2 gap-1" style="background-color: {theme.colors.background}">
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.red}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.green}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.yellow}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.blue}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.magenta}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.cyan}"></span>
            </div>
            <span class="text-xs font-medium text-surface-700 dark:text-surface-300">{theme.name}</span>
          </button>
        {/each}
      </div>
    </section>

    <!-- Light Terminal Theme -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Light Terminal Theme</h2>
      <div class="grid grid-cols-3 gap-3">
        {#each lightThemes as theme (theme.id)}
          <button
            class="rounded-lg border-2 p-3 text-left transition-colors {config.appearance.terminal_theme_light === theme.id ? 'border-primary-500' : 'border-surface-200 dark:border-surface-700 hover:border-surface-400 dark:hover:border-surface-500'}"
            onclick={() => updateSettings({ appearance: { ...config.appearance, terminal_theme_light: theme.id } })}
          >
            <div class="rounded h-16 mb-2 flex items-end p-2 gap-1" style="background-color: {theme.colors.background}">
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.red}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.green}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.yellow}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.blue}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.magenta}"></span>
              <span class="w-3 h-3 rounded-full" style="background-color: {theme.colors.cyan}"></span>
            </div>
            <span class="text-xs font-medium text-surface-700 dark:text-surface-300">{theme.name}</span>
          </button>
        {/each}
      </div>
    </section>

    <!-- Dark Diff Editor Theme -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Dark Diff Editor Theme</h2>
      <div class="grid grid-cols-3 gap-3">
        {#each [{ id: "vs-dark", name: "VS Dark", bg: "#1e1e1e", added: "#2ea04370", removed: "#f8514970" }, { id: "hc-black", name: "High Contrast", bg: "#000000", added: "#2ea04390", removed: "#f8514990" }] as theme (theme.id)}
          <button
            class="rounded-lg border-2 p-3 text-left transition-colors {config.appearance.diff_theme_dark === theme.id ? 'border-primary-500' : 'border-surface-200 dark:border-surface-700 hover:border-surface-400 dark:hover:border-surface-500'}"
            onclick={() => updateSettings({ appearance: { ...config.appearance, diff_theme_dark: theme.id } })}
          >
            <div class="rounded h-16 mb-2 flex flex-col justify-center px-2 gap-0.5 text-[9px] font-mono" style="background-color: {theme.bg}">
              <div class="rounded px-1" style="background-color: {theme.removed}"><span class="text-red-300">- removed</span></div>
              <div class="rounded px-1" style="background-color: {theme.added}"><span class="text-green-300">+ added</span></div>
            </div>
            <span class="text-xs font-medium text-surface-700 dark:text-surface-300">{theme.name}</span>
          </button>
        {/each}
      </div>
    </section>

    <!-- Light Diff Editor Theme -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Light Diff Editor Theme</h2>
      <div class="grid grid-cols-3 gap-3">
        {#each [{ id: "vs", name: "VS Light", bg: "#ffffff", added: "#2ea04330", removed: "#f8514930" }, { id: "hc-light", name: "High Contrast", bg: "#ffffff", added: "#2ea04360", removed: "#f8514960" }] as theme (theme.id)}
          <button
            class="rounded-lg border-2 p-3 text-left transition-colors {config.appearance.diff_theme_light === theme.id ? 'border-primary-500' : 'border-surface-200 dark:border-surface-700 hover:border-surface-400 dark:hover:border-surface-500'}"
            onclick={() => updateSettings({ appearance: { ...config.appearance, diff_theme_light: theme.id } })}
          >
            <div class="rounded h-16 mb-2 flex flex-col justify-center px-2 gap-0.5 text-[9px] font-mono border border-surface-200" style="background-color: {theme.bg}">
              <div class="rounded px-1" style="background-color: {theme.removed}"><span class="text-red-700">- removed</span></div>
              <div class="rounded px-1" style="background-color: {theme.added}"><span class="text-green-700">+ added</span></div>
            </div>
            <span class="text-xs font-medium text-surface-700 dark:text-surface-300">{theme.name}</span>
          </button>
        {/each}
      </div>
    </section>

    <!-- Font Size -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Font Size</h2>
      <div class="flex items-center gap-3">
        <button
          class="rounded border border-surface-300 dark:border-surface-600 px-3 py-1.5 text-sm text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800"
          onclick={() => setFontSize(config.terminal.font_size - 1)}
        >−</button>
        <span class="text-lg font-mono text-surface-900 dark:text-surface-50 w-8 text-center">{config.terminal.font_size}</span>
        <button
          class="rounded border border-surface-300 dark:border-surface-600 px-3 py-1.5 text-sm text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800"
          onclick={() => setFontSize(config.terminal.font_size + 1)}
        >+</button>
        <span class="text-xs text-surface-700 dark:text-surface-300">px (8–32)</span>
      </div>
    </section>

    <!-- Font Family -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Font Family</h2>
      <Select
        items={fontItems}
        value={config.terminal.font_family}
        onValueChange={(v) => updateSettings({ terminal: { ...config.terminal, font_family: v } })}
        placeholder="Search fonts…"
      />
      <p class="text-xs text-surface-700 dark:text-surface-300" style="font-family: '{config.terminal.font_family}', monospace">The quick brown fox jumps over the lazy dog</p>
    </section>

    <!-- Option as Meta (macOS only) -->
    {#if IS_MAC}
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Option as Meta</h2>
      <label class="flex items-center gap-3 cursor-pointer">
        <input
          type="checkbox"
          checked={config.terminal.option_as_meta}
          onchange={(e) => updateSettings({ terminal: { ...config.terminal, option_as_meta: e.currentTarget.checked } })}
          class="w-4 h-4 rounded border-surface-300 dark:border-surface-600 text-primary-500 focus:ring-primary-500"
        />
        <span class="text-sm text-surface-700 dark:text-surface-300">Send Option key as Meta/Escape (required for tmux Alt-key bindings)</span>
      </label>
    </section>
    {/if}

    <!-- Session Backend -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Session Backend</h2>
      <div class="flex gap-2">
        {#each [{ value: "auto", label: "Auto" }, { value: "tmux", label: "tmux" }, { value: "direct", label: "Direct" }] as opt (opt.value)}
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {backendValue === opt.value ? 'bg-primary-500 text-white' : 'bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700'}"
            onclick={() => setSessionBackend(opt.value)}
          >{opt.label}</button>
        {/each}
      </div>
      {#if backendValue === "tmux" && !tmuxAvailable}
        <p class="text-xs text-amber-600 dark:text-amber-400">⚠ tmux not found on PATH. Sessions will fail to launch.</p>
      {/if}
      <p class="text-xs text-surface-700 dark:text-surface-400">
        {#if backendValue === "auto"}Auto-detect: uses tmux if available, otherwise direct PTY.
        {:else if backendValue === "tmux"}Sessions persist after quitting (requires tmux).
        {:else}Sessions are ephemeral — terminated on app quit.
        {/if}
        Changes apply to new sessions only.
      </p>
    </section>

    <!-- Providers -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Providers</h2>
      <div class="space-y-3">
        {#each Object.entries(config.providers) as [key, provider] (key)}
          <div class="rounded-lg border border-surface-200 dark:border-surface-700 p-4 space-y-2">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-surface-900 dark:text-surface-50">{key}</span>
                {#if config.default_provider === key}
                  <span class="text-xs bg-primary-500/20 text-primary-700 dark:text-primary-300 px-2 py-0.5 rounded">default</span>
                {:else}
                  <button
                    class="text-xs text-surface-500 hover:text-primary-500"
                    onclick={() => setDefaultProvider(key)}
                  >set as default</button>
                {/if}
              </div>
              <button
                class="text-xs text-red-500 hover:text-red-700"
                onclick={() => removeProvider(key)}
                disabled={Object.keys(config.providers).length <= 1}
              >Remove</button>
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400">Command</label>
              <Input
                value={provider.command}
                onchange={(e) => updateProvider(key, "command", e.currentTarget.value)}
                class="font-mono"
              />
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400">Yolo flag (optional)</label>
              <Input
                value={provider.yolo_flag || ""}
                onchange={(e) => updateProvider(key, "yolo_flag", e.currentTarget.value)}
                class="font-mono"
                placeholder="e.g. --trust-all-tools"
              />
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">Prompt command (optional) <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 whitespace-nowrap rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Variable: {"{prompt}"} — replaced with rendered task prompt</span></span></label>
              <Input
                value={provider.prompt_command || ""}
                onchange={(e) => updateProvider(key, "prompt_command", e.currentTarget.value)}
                class="font-mono"
                placeholder={"{prompt} or --prompt {prompt}"}
              />
            </div>
          </div>
        {/each}
      </div>

      {#if showAddProvider}
        <div class="rounded-lg border border-primary-300 dark:border-primary-700 p-4 space-y-2">
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">Name</label>
            <Input
              bind:value={newProviderName}
              placeholder="e.g. claude-code"
            />
          </div>
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">Command</label>
            <Input
              bind:value={newProviderCommand}
              class="font-mono"
              placeholder="e.g. claude"
            />
          </div>
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">Yolo flag (optional)</label>
            <Input
              bind:value={newProviderYoloFlag}
              class="font-mono"
              placeholder="e.g. --dangerously-skip-permissions"
            />
          </div>
          <div class="flex gap-2">
            <button
              class="px-3 py-1.5 rounded text-sm font-medium bg-primary-500 text-white hover:bg-primary-600 disabled:opacity-50"
              onclick={addProvider}
              disabled={!newProviderName || !newProviderCommand}
            >Add</button>
            <button
              class="px-3 py-1.5 rounded text-sm font-medium bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700"
              onclick={() => { showAddProvider = false; }}
            >Cancel</button>
          </div>
        </div>
      {:else}
        <button
          class="px-4 py-2 rounded-md text-sm font-medium bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700"
          onclick={() => { showAddProvider = true; }}
        >+ Add Provider</button>
      {/if}
    </section>

    <!-- Task Managers -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Task Manager</h2>
      <div class="space-y-3">
        {#each Object.entries(taskManagers) as [key, tm] (key)}
          <div class="rounded-lg border border-surface-200 dark:border-surface-700 p-4 space-y-2">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-surface-900 dark:text-surface-50">{key}</span>
                {#if config.default_task_manager === key}
                  <span class="text-xs bg-primary-500/20 text-primary-700 dark:text-primary-300 px-2 py-0.5 rounded">default</span>
                {:else}
                  <button class="text-xs text-surface-500 hover:text-primary-500" onclick={() => setDefaultTaskManager(key)}>set as default</button>
                {/if}
              </div>
              <button class="text-xs text-red-500 hover:text-red-700" onclick={() => removeTaskManager(key)}>Remove</button>
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">Get task command <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Fetches a single task by key. Must output JSON. Variables: {"{key}"}</span></span></label>
              <Input value={tm.get_task} onchange={(e) => updateTaskManager(key, "get_task", e.currentTarget.value)} class="font-mono" placeholder={"kanban show {key}"} />
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">Move task command <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Moves a task to a new status. Called by lifecycle hooks. Variables: {"{key}"}, {"{status}"}</span></span></label>
              <Input value={tm.move_task} onchange={(e) => updateTaskManager(key, "move_task", e.currentTarget.value)} class="font-mono" placeholder={"kanban move {key} {status}"} />
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">List tasks command <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Lists tasks for the task picker. Must output a JSON array. Runs with project path as CWD.</span></span></label>
              <Input value={tm.list_tasks} onchange={(e) => updateTaskManager(key, "list_tasks", e.currentTarget.value)} class="font-mono" placeholder="kanban list --status todo" />
            </div>

            <!-- Templates -->
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">Branch template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Git branch name created for the session. Variables: {"{key}"}, {"{title}"}, {"{status}"}, {"{description}"}, {"{priority}"}, {"{blocked_by}"}. Transforms: :slug, :lower, :upper</span></span></label>
              <Input value={tm.templates?.branch || "{key:lower}/{title:slug}"} onchange={(e) => updateTmTemplate(key, "branch", e.currentTarget.value)} class="font-mono" />
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">Session name template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Display name shown in sidebar and tab bar. Variables: {"{key}"}, {"{title}"}, {"{status}"}, {"{description}"}, {"{priority}"}, {"{blocked_by}"}. Transforms: :slug, :lower, :upper</span></span></label>
              <Input value={tm.templates?.name || "{key:upper}: {title}"} onchange={(e) => updateTmTemplate(key, "name", e.currentTarget.value)} class="font-mono" />
            </div>
            <div class="space-y-1">
              <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">Prompt template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">Initial prompt sent to the agent via prompt_command. Variables: {"{key}"}, {"{title}"}, {"{status}"}, {"{description}"}, {"{priority}"}, {"{blocked_by}"}. Transforms: :slug, :lower, :upper</span></span></label>
              <Input value={tm.templates?.prompt || "Implement task {key}: {title}\n\n{description}"} onchange={(e) => updateTmTemplate(key, "prompt", e.currentTarget.value)} class="font-mono" />
            </div>

            <!-- Lifecycle hooks -->
            {#each [["on_start", "On start", "in_progress", "Fires when a session is created from this task."], ["on_notify", "On notify", "in_review", "Fires when the agent signals idle (task complete notification)."], ["on_restart", "On restart", "in_progress", "Fires when an exited task-linked session is restarted."], ["on_complete", "On complete", "done", "Fires when a task-linked session is archived or deleted."]] as [hookKey, label, defaultVal, desc]}
              <div class="space-y-1">
                <label class="text-xs text-surface-700 dark:text-surface-400 flex items-center gap-1">{label} → move to <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-surface-800 dark:bg-surface-200 text-surface-50 dark:text-surface-900 px-2 py-1 text-[10px]">{desc}</span></span></label>
                <Input value={(tm as any)[hookKey]?.move_to || defaultVal} onchange={(e) => updateTmHook(key, hookKey, e.currentTarget.value)} class="font-mono" />
              </div>
            {/each}
          </div>
        {/each}
      </div>

      {#if showAddTaskManager}
        <div class="rounded-lg border border-primary-300 dark:border-primary-700 p-4 space-y-2">
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">Name</label>
            <Input bind:value={newTmName} placeholder="e.g. kanban" />
          </div>
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">Get task command</label>
            <Input bind:value={newTmGetTask} class="font-mono" />
          </div>
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">Move task command</label>
            <Input bind:value={newTmMoveTask} class="font-mono" />
          </div>
          <div class="space-y-1">
            <label class="text-xs text-surface-700 dark:text-surface-400">List tasks command</label>
            <Input bind:value={newTmListTasks} class="font-mono" />
          </div>
          <div class="flex gap-2">
            <button class="px-3 py-1.5 rounded text-sm font-medium bg-primary-500 text-white hover:bg-primary-600 disabled:opacity-50" onclick={addTaskManager} disabled={!newTmName || !newTmGetTask || !newTmMoveTask || !newTmListTasks}>Add</button>
            <button class="px-3 py-1.5 rounded text-sm font-medium bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700" onclick={() => { showAddTaskManager = false; }}>Cancel</button>
          </div>
        </div>
      {:else}
        <button class="px-4 py-2 rounded-md text-sm font-medium bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700" onclick={() => { showAddTaskManager = true; }}>+ Add Task Manager</button>
      {/if}
    </section>
  </div>
</div>
