<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { preferences } from "../lib/api";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { loadSettings, getSettings, updateSettings, refreshSettings, type AppearanceMode, type AppConfig, type Provider, type TaskManager } from "../lib/settings.svelte";
  import { loadTheme } from "../lib/theme-loader";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { Select, Input, Button, Dialog } from "./ui";
  import { Palette, Bot, ListTodo, Settings, Cable, RefreshCw, Puzzle } from "@lucide/svelte";
  import JiraSettings from "./JiraSettings.svelte";
  import PluginManager from "./PluginManager.svelte";

  const config = $derived(getSettings());
  let fontItems = $state<{ value: string; label: string }[]>([]);
  let availableThemes = $state<string[]>([]);
  let editingProvider = $state<string | null>(null);
  let newProviderName = $state("");
  let newProviderCommand = $state("");
  let newProviderYoloFlag = $state("");
  let showAddProvider = $state(false);
  let tmuxAvailable = $state(true);
  let cliInstalled = $state(false);
  let activeTab = $state("Appearance");

  let staleWorktrees = $state<{ session_name: string; worktree_path: string; branch: string }[]>([]);
  let showCleanupDialog = $state(false);
  let cleanupMessage = $state("");

  async function triggerCleanupPreview() {
    cleanupMessage = "";
    try {
      const items = await preferences.listStaleWorktrees();
      if (items.length === 0) {
        cleanupMessage = "No stale worktrees found.";
      } else {
        staleWorktrees = items;
        showCleanupDialog = true;
      }
    } catch (e) {
      cleanupMessage = `Failed to list worktrees: ${e}`;
    }
  }

  async function confirmCleanup() {
    try {
      const errors = await preferences.runStaleWorktreeCleanup();
      cleanupMessage = errors.length
        ? `Cleanup finished with ${errors.length} error(s).`
        : "Cleanup complete.";
    } catch (e) {
      cleanupMessage = `Cleanup failed: ${e}`;
    } finally {
      showCleanupDialog = false;
      staleWorktrees = [];
    }
  }

  const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  const fontDropdownItems = $derived(
    fontItems.length > 0
      ? fontItems
      : config.terminal.font_family
        ? [{ value: config.terminal.font_family, label: config.terminal.font_family }]
        : []
  );

  function handleKeydown(e: KeyboardEvent) {
    const mod = IS_MAC ? e.metaKey : e.ctrlKey;
    if (mod && e.key.toLowerCase() === "w") {
      e.preventDefault();
      e.stopPropagation();
      getCurrentWindow().close();
    }
  }

  onMount(async () => {
    window.addEventListener("keydown", handleKeydown, true);
    await loadSettings();
    loadTheme();
    preferences.listMonospaceFonts().then((fonts) => {
      fontItems = fonts.map((f) => ({ value: f, label: f }));
    });
    preferences.listThemes().then((themes) => {
      availableThemes = themes;
    });
    preferences.checkTmuxAvailable().then((available) => {
      tmuxAvailable = available;
    });
    preferences.checkCliInstalled().then((installed) => {
      cliInstalled = installed;
    });
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeydown, true);
  });

  const backendValue = $derived(config.session_backend ?? "auto");
  const vimEnabled = $derived(config.vim_mode ?? true);

  function setSessionBackend(value: string) {
    const backend = value === "auto" ? null : value;
    updateSettings({ session_backend: backend } as Partial<AppConfig>);
  }

  function setVimMode(enabled: boolean) {
    updateSettings({ vim_mode: enabled } as Partial<AppConfig>);
  }

  async function pickProjectsBasePath() {
    const selected = await open({ directory: true, multiple: false, defaultPath: config.projects_base_path ?? undefined });
    if (selected) {
      updateSettings({ projects_base_path: selected as string } as Partial<AppConfig>);
    }
  }

  async function installCli() {
    try {
      await preferences.installCli();
      cliInstalled = true;
    } catch (e) {
      console.error("Failed to install CLI:", e);
    }
  }

  async function openLogFolder() {
    const logDir = await preferences.getLogDir();
    await revealItemInDir(logDir);
  }

  async function refreshConfig() {
    try {
      await refreshSettings();
      showSnackbar("Config reloaded from disk", "success");
    } catch (e) {
      showSnackbar(`Failed to refresh config: ${e}`, "error");
    }
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

  // Task management state
  const taskManagement = $derived(config.task_management ?? null);

  function enableTaskManagement() {
    updateSettings({ task_management: {
      templates: { branch: "{key:lower}/{title:slug}", name: "{key:upper}: {title}", prompt: "Implement task {key}: {title}\n\n{description}" },
      on_start: { move_to: "in_progress" },
      on_notify: { move_to: "in_review" },
      on_restart: { move_to: "in_progress" },
      on_complete: { move_to: "done" },
    } } as Partial<AppConfig>);
  }

  function updateTmField(field: string, value: any) {
    const tm = { ...config.task_management };
    (tm as any)[field] = value || undefined;
    updateSettings({ task_management: tm } as Partial<AppConfig>);
  }

  function updateTmTemplate(field: string, value: string) {
    const tm = { ...config.task_management };
    const templates = { ...tm.templates };
    (templates as any)[field] = value || null;
    tm.templates = templates;
    updateSettings({ task_management: tm } as Partial<AppConfig>);
  }

  function updateTmHook(hookName: string, value: string) {
    const tm = { ...config.task_management };
    (tm as any)[hookName] = value ? { move_to: value } : null;
    updateSettings({ task_management: tm } as Partial<AppConfig>);
  }

  function updateAutoDispatch(patch: Partial<import("../lib/settings.svelte").AutoDispatchConfig>) {
    const tm = { ...config.task_management };
    tm.auto_dispatch = { ...tm.auto_dispatch, ...patch };
    updateSettings({ task_management: tm } as Partial<AppConfig>);
  }
</script>

<div class="h-screen flex flex-col overflow-hidden bg-panel">
  <nav class="flex justify-center gap-1 border-b border-border px-8 pt-4">
    {#each [{name: "Appearance", icon: Palette}, {name: "Models", icon: Bot}, {name: "Task Management", icon: ListTodo}, {name: "Integrations", icon: Cable}, {name: "Plugins", icon: Puzzle}, {name: "More", icon: Settings}] as tab (tab.name)}
      <button
        class="flex items-center gap-1.5 px-4 py-2 text-[13px] font-medium transition-colors border-b-2 -mb-px {activeTab === tab.name ? 'border-accent text-accent' : 'border-transparent text-t3 hover:text-t1'}"
        onclick={() => activeTab = tab.name}
      ><tab.icon size={15} />{tab.name}</button>
    {/each}
  </nav>
  <div class="flex-1 overflow-y-auto px-8 py-6">
  <div class="max-w-[564px] mx-auto space-y-8">

    {#if activeTab === "Appearance"}
    <!-- Appearance Mode -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Appearance</h2>
      <div class="inline-flex gap-1 rounded-lg bg-panel-hi p-0.5">
        {#each ["system", "light", "dark"] as mode (mode)}
          <button
            class="px-4 py-2 rounded-md text-[12px] font-medium capitalize transition-colors {config.appearance.mode === mode ? 'bg-accent text-on-accent' : 'text-t2 hover:text-t1'}"
            onclick={() => setAppearance(mode as AppearanceMode)}
          >{mode}</button>
        {/each}
      </div>
    </section>

    <!-- Theme -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Theme</h2>
      <div class="flex flex-wrap gap-2">
        {#each availableThemes as themeName (themeName)}
          <button
            class="px-3 py-1.5 rounded-lg text-[12px] font-medium capitalize transition-colors {config.appearance.theme === themeName ? 'bg-accent/15 text-accent ring-1 ring-accent/30' : 'bg-panel-hi text-t2 hover:text-t1'}"
            onclick={() => updateSettings({ appearance: { ...config.appearance, theme: themeName } })}
          >{themeName}</button>
        {/each}
      </div>
    </section>



    <!-- Font Size -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Font Size</h2>
      <div class="flex items-center gap-3">
        <button
          class="rounded border border-border px-3 py-1.5 text-sm text-t1 hover:bg-panel-hi"
          onclick={() => setFontSize(config.terminal.font_size - 1)}
        >−</button>
        <span class="text-lg font-mono text-t1 w-8 text-center">{config.terminal.font_size}</span>
        <button
          class="rounded border border-border px-3 py-1.5 text-sm text-t1 hover:bg-panel-hi"
          onclick={() => setFontSize(config.terminal.font_size + 1)}
        >+</button>
        <span class="text-xs text-t1">px (8–32)</span>
      </div>
    </section>

    <!-- Font Family -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Font Family</h2>
      <Select
        items={fontDropdownItems}
        value={config.terminal.font_family}
        onValueChange={(v) => updateSettings({ terminal: { ...config.terminal, font_family: v } })}
        placeholder="Search fonts…"
      />
      <p class="text-xs text-t1" style="font-family: '{config.terminal.font_family}', monospace">The quick brown fox jumps over the lazy dog</p>
    </section>

    <!-- Option as Meta (macOS only) -->
    {#if IS_MAC}
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Option as Meta</h2>
      <label class="flex items-center gap-3 cursor-pointer">
        <input
          type="checkbox"
          checked={config.terminal.option_as_meta}
          onchange={(e) => updateSettings({ terminal: { ...config.terminal, option_as_meta: e.currentTarget.checked } })}
          class="w-4 h-4 rounded border-border text-accent focus:ring-accent"
        />
        <span class="text-sm text-t1">Send Option key as Meta/Escape (required for tmux Alt-key bindings)</span>
      </label>
    </section>
    {/if}

    {/if}

    {#if activeTab === "Models"}
    <!-- Providers -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Providers</h2>
      <div class="space-y-3">
        {#each Object.entries(config.providers) as [key, provider] (key)}
          <div class="rounded-lg border border-border p-4 space-y-3">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-t1">{key}</span>
                {#if config.default_provider === key}
                  <span class="text-xs bg-accent/20 text-accent px-2 py-0.5 rounded">default</span>
                {:else}
                  <button
                    class="text-xs text-t3 hover:text-accent"
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
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2">Command</label>
              <Input
                value={provider.command}
                onchange={(e) => updateProvider(key, "command", e.currentTarget.value)}
                class="font-mono"
              />
            </div>
            <div class="space-y-1">
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2">Yolo flag (optional)</label>
              <Input
                value={provider.yolo_flag || ""}
                onchange={(e) => updateProvider(key, "yolo_flag", e.currentTarget.value)}
                class="font-mono"
                placeholder="e.g. --trust-all-tools"
              />
            </div>
            <div class="space-y-1">
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2 flex items-center gap-1">Interactive resume command (optional) <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Command run when restarting an exited session on focus. Falls back to the base command if empty.</span></span></label>
              <Input
                value={provider.resume_command || ""}
                onchange={(e) => updateProvider(key, "resume_command", e.currentTarget.value)}
                class="font-mono"
                placeholder="e.g. kiro-cli chat --trust-all-tools --resume-picker"
              />
            </div>
            <div class="space-y-1">
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2 flex items-center gap-1">Prompt command (optional) <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 whitespace-nowrap rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Variable: {"{prompt}"} — replaced with rendered task prompt</span></span></label>
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
        <div class="rounded-lg border border-accent p-4 space-y-2">
          <div class="space-y-1">
            <!-- svelte-ignore a11y_label_has_associated_control -->
            <label class="text-xs text-t2">Name</label>
            <Input
              bind:value={newProviderName}
              placeholder="e.g. claude-code"
            />
          </div>
          <div class="space-y-1">
            <!-- svelte-ignore a11y_label_has_associated_control -->
            <label class="text-xs text-t2">Command</label>
            <Input
              bind:value={newProviderCommand}
              class="font-mono"
              placeholder="e.g. claude"
            />
          </div>
          <div class="space-y-1">
            <!-- svelte-ignore a11y_label_has_associated_control -->
            <label class="text-xs text-t2">Yolo flag (optional)</label>
            <Input
              bind:value={newProviderYoloFlag}
              class="font-mono"
              placeholder="e.g. --dangerously-skip-permissions"
            />
          </div>

          <div class="flex gap-2">
            <button
              class="px-3 py-1.5 rounded text-sm font-medium bg-accent text-on-accent hover:opacity-90 disabled:opacity-50"
              onclick={addProvider}
              disabled={!newProviderName || !newProviderCommand}
            >Add</button>
            <button
              class="px-3 py-1.5 rounded text-sm font-medium bg-panel-hi text-t1 hover:bg-panel-hi"
              onclick={() => { showAddProvider = false; }}
            >Cancel</button>
          </div>
        </div>
      {:else}
        <button
          class="px-4 py-2 rounded-md text-sm font-medium bg-panel-hi text-t1 hover:bg-panel-hi"
          onclick={() => { showAddProvider = true; }}
        >+ Add Provider</button>
      {/if}
    </section>

    {/if}

    {#if activeTab === "Task Management"}
    <!-- PR Integration -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Pull Request Integration</h2>
      <div class="space-y-1">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-xs text-t2 flex items-center gap-1">PR status command <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Command to check PR status for a branch. Must output JSON with "url" and "state" (open/merged/closed). Non-zero exit = no PR. Variable: {"{branch}"}</span></span></label>
        <Input value={config.pr_status || ""} onchange={(e) => updateSettings({ pr_status: e.currentTarget.value || null } as Partial<AppConfig>)} class="font-mono" placeholder={"gh pr view {branch} --json url,state,isDraft"} />
      </div>
    </section>

    <!-- Task Manager -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Task Manager</h2>
      {#if taskManagement}
        <div class="rounded-lg border border-border p-4 space-y-2">
            <!-- Templates -->
            <div class="space-y-1">
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2 flex items-center gap-1">Branch template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Git branch name created for the session. Variables: {"{key}"}, {"{parent_key}"} (falls back to key), {"{title}"}, {"{status}"}, {"{description}"}, {"{priority}"}, {"{blocked_by}"}. Transforms: :slug, :lower, :upper</span></span></label>
              <Input value={taskManagement.templates?.branch || "{key:lower}/{title:slug}"} onchange={(e) => updateTmTemplate("branch", e.currentTarget.value)} class="font-mono" />
            </div>
            <div class="space-y-1">
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2 flex items-center gap-1">Session name template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Display name shown in sidebar and tab bar. Variables: {"{key}"}, {"{title}"}, {"{status}"}, {"{description}"}, {"{priority}"}, {"{blocked_by}"}. Transforms: :slug, :lower, :upper</span></span></label>
              <Input value={taskManagement.templates?.name || "{key:upper}: {title}"} onchange={(e) => updateTmTemplate("name", e.currentTarget.value)} class="font-mono" />
            </div>
            <div class="space-y-1">
              <!-- svelte-ignore a11y_label_has_associated_control -->
              <label class="text-xs text-t2 flex items-center gap-1">Prompt template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Initial prompt sent to the agent via prompt_command. Variables: {"{key}"}, {"{title}"}, {"{status}"}, {"{description}"}, {"{priority}"}, {"{blocked_by}"}. Transforms: :slug, :lower, :upper</span></span></label>
              <Input value={taskManagement.templates?.prompt || "Implement task {key}: {title}\n\n{description}"} onchange={(e) => updateTmTemplate("prompt", e.currentTarget.value)} class="font-mono" />
            </div>

            <!-- Lifecycle hooks -->
            <div class="space-y-2 pt-2 mt-2 border-t border-border">
              <span class="text-[10px] font-semibold text-t3 uppercase tracking-wider">Lifecycle Hooks</span>
              {#each [["on_start", "On start", "in_progress", "Fires when a session is created from this task."], ["on_notify", "On notify", "in_review", "Fires when the agent signals idle (task complete notification)."], ["on_restart", "On restart", "in_progress", "Fires when an exited task-linked session is restarted."], ["on_complete", "On complete", "done", "Fires when a task-linked session is archived or deleted."], ["on_pr_open", "On PR open", "in_review", "Fires when a pull request is opened for this session's branch."], ["on_pr_merge", "On PR merge", "done", "Fires when the pull request is merged."]] as [hookKey, label, defaultVal, desc]}
                <div class="flex items-center gap-2">
                  <!-- svelte-ignore a11y_label_has_associated_control -->
                  <label class="text-xs text-t2 w-24 shrink-0 flex items-center gap-1">{label} <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">{desc}</span></span></label>
                  <span class="text-xs text-t3">→</span>
                  <Input value={(taskManagement as any)[hookKey]?.move_to || defaultVal} onchange={(e) => updateTmHook(hookKey, e.currentTarget.value)} class="font-mono flex-1" />
                </div>
              {/each}
            </div>

            <!-- Auto-dispatch -->
            <div class="mt-3 pt-3 border-t border-border space-y-2">
              <div class="flex items-center gap-2">
                <label class="text-xs font-medium text-t1 flex items-center gap-1">
                  <input type="checkbox" class="rounded" checked={!!taskManagement.auto_dispatch} onchange={(e) => {
                    const tm = { ...config.task_management };
                    if ((e.currentTarget as HTMLInputElement).checked) {
                      tm.auto_dispatch = { poll_interval_ms: 30000, max_concurrent: 3 };
                    } else {
                      tm.auto_dispatch = null;
                    }
                    updateSettings({ task_management: tm } as Partial<AppConfig>);
                  }} />
                  Auto-dispatch
                </label>
                <span class="text-[10px] text-t3">Automatically spawn sessions for tasks</span>
              </div>
              {#if taskManagement.auto_dispatch}
                <div class="grid grid-cols-2 gap-2">
                  <div class="space-y-1">
                    <!-- svelte-ignore a11y_label_has_associated_control -->
                    <label class="text-xs text-t2">Poll interval (ms)</label>
                    <Input value={String(taskManagement.auto_dispatch.poll_interval_ms ?? 30000)} onchange={(e) => {
                      updateAutoDispatch({ poll_interval_ms: parseInt(e.currentTarget.value) || 30000 });
                    }} class="font-mono" />
                  </div>
                  <div class="space-y-1">
                    <!-- svelte-ignore a11y_label_has_associated_control -->
                    <label class="text-xs text-t2">Max concurrent</label>
                    <Input value={String(taskManagement.auto_dispatch.max_concurrent ?? 3)} onchange={(e) => {
                      updateAutoDispatch({ max_concurrent: parseInt(e.currentTarget.value) || 3 });
                    }} class="font-mono" />
                  </div>
                </div>
                <div class="space-y-1">
                  <!-- svelte-ignore a11y_label_has_associated_control -->
                  <label class="text-xs text-t2 flex items-center gap-1">Provider <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Which agent to use for auto-dispatched sessions. Leave empty to use default provider.</span></span></label>
                  <Input value={taskManagement.auto_dispatch.provider || ""} onchange={(e) => {
                    updateAutoDispatch({ provider: e.currentTarget.value || undefined });
                  }} class="font-mono" placeholder={config.default_provider} />
                </div>
                <div class="space-y-1">
                  <!-- svelte-ignore a11y_label_has_associated_control -->
                  <label class="text-xs text-t2 flex items-center gap-1">Terminal states <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-56 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Comma-separated. Tasks in these states are considered finished and won't be dispatched. Running sessions are killed if their task enters a terminal state.</span></span></label>
                  <Input value={(taskManagement.auto_dispatch.terminal_states ?? ["done", "cancelled"]).join(", ")} onchange={(e) => {
                    updateAutoDispatch({ terminal_states: e.currentTarget.value.split(",").map((s: string) => s.trim()).filter(Boolean) });
                  }} class="font-mono" placeholder="done, cancelled" />
                </div>
                <div class="space-y-1">
                  <!-- svelte-ignore a11y_label_has_associated_control -->
                  <label class="text-xs text-t2 flex items-center gap-1">Autonomous prompt template <span class="relative group cursor-help">ⓘ<span class="hidden group-hover:block absolute left-4 top-0 z-50 w-64 whitespace-normal rounded bg-panel-hi text-t3 px-2 py-1 text-[10px]">Wraps the task prompt for auto-dispatched sessions. Variable: {"{prompt}"} is replaced with the rendered task prompt. Leave empty to use the task prompt as-is.</span></span></label>
                  <Input
                    value={taskManagement.auto_dispatch.autonomous_prompt_template || ""}
                    onchange={(e) => updateAutoDispatch({ autonomous_prompt_template: e.currentTarget.value || null })}
                    class="font-mono"
                    placeholder="e.g. Be autonomous.\n{prompt}"
                  />
                </div>
              {/if}
            </div>
          </div>

          <button class="px-4 py-2 rounded-md text-sm font-medium bg-red-500/10 text-red-600 hover:bg-red-500/20" onclick={() => updateSettings({ task_management: null } as Partial<AppConfig>)}>Disable Task Management</button>
      {:else}
        <button class="px-4 py-2 rounded-md text-sm font-medium bg-panel-hi text-t1 hover:bg-panel-hi" onclick={enableTaskManagement}>Enable Task Management</button>
      {/if}
    </section>
    {/if}

    {#if activeTab === "Integrations"}
    <JiraSettings />
    {/if}

    {#if activeTab === "Plugins"}
    <PluginManager />
    {/if}

    {#if activeTab === "More"}
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Config File</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Reload config from disk</p>
          <p class="text-xs text-t3">Re-read ~/.config/planeai/config.json and apply changes.</p>
        </div>
        <Button type="button" onclick={refreshConfig}><RefreshCw size={14} class="inline mr-1" />Refresh</Button>
      </div>
    </section>

    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Session Backend</h2>
      <div class="flex gap-2">
        {#each [{ value: "local", label: "Local" }, { value: "tmux", label: "tmux" }, { value: "daemon", label: "Daemon (experimental)" }] as opt (opt.value)}
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {backendValue === opt.value ? 'bg-accent text-on-accent' : 'bg-panel-hi text-t1 hover:bg-panel-hi '}"
            onclick={() => setSessionBackend(opt.value)}
          >{opt.label}</button>
        {/each}
      </div>
      {#if backendValue === "tmux" && !tmuxAvailable}
        <p class="text-xs text-amber-600">⚠ tmux not found on PATH. Sessions will fail to launch.</p>
      {/if}
      <p class="text-xs text-t2">
        {#if backendValue === "local"}Sessions run in-process. Restarted with resume command on focus.
        {:else if backendValue === "tmux"}Sessions persist after quitting (requires tmux).
        {:else if backendValue === "daemon"}Sessions persist after quitting (built-in daemon, experimental).
        {:else}Sessions run in-process. Restarted with resume command on focus.
        {/if}
        Changes apply to new sessions only.
      </p>
    </section>

    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Vim Mode</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Enable vim keybindings</p>
          <p class="text-xs text-t3">Full vim emulation in the code editor (motions, visual mode, ex commands)</p>
        </div>
        <button
          class="w-10 h-5 rounded-full transition-colors {vimEnabled ? 'bg-accent' : 'bg-panel-hi'}"
          onclick={() => setVimMode(!vimEnabled)}
          role="switch"
          aria-checked={vimEnabled}
          aria-label="Toggle vim mode"
        >
          <span class="block w-4 h-4 rounded-full bg-white shadow transition-transform {vimEnabled ? 'translate-x-5' : 'translate-x-0.5'}"></span>
        </button>
      </div>
    </section>

    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Auto-open Review</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Auto-open review when agent finishes</p>
          <p class="text-xs text-t3">Automatically open the Review tab when the active session's agent stops and has changes.</p>
        </div>
        <button
          class="w-10 h-5 rounded-full transition-colors {(config.auto_open_review ?? true) ? 'bg-accent' : 'bg-panel-hi'}"
          onclick={() => updateSettings({ auto_open_review: !(config.auto_open_review ?? true) } as Partial<AppConfig>)}
          role="switch"
          aria-checked={config.auto_open_review ?? true}
          aria-label="Toggle auto-open review"
        >
          <span class="block w-4 h-4 rounded-full bg-white shadow transition-transform {(config.auto_open_review ?? true) ? 'translate-x-5' : 'translate-x-0.5'}"></span>
        </button>
      </div>
    </section>

    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Sound</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Play sound notifications</p>
          <p class="text-xs text-t3">Play a chime when an agent finishes a task.</p>
        </div>
        <button
          class="w-10 h-5 rounded-full transition-colors {(config.sound_enabled ?? true) ? 'bg-accent' : 'bg-panel-hi'}"
          onclick={() => updateSettings({ sound_enabled: !(config.sound_enabled ?? true) } as Partial<AppConfig>)}
          role="switch"
          aria-checked={config.sound_enabled ?? true}
          aria-label="Toggle sound notifications"
        >
          <span class="block w-4 h-4 rounded-full bg-white shadow transition-transform {(config.sound_enabled ?? true) ? 'translate-x-5' : 'translate-x-0.5'}"></span>
        </button>
      </div>
    </section>

    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Post-Merge Action</h2>
      <Select
        items={[{ value: "archive", label: "Archive" }, { value: "destroy", label: "Destroy" }, { value: "keep", label: "Keep" }]}
        value={config.post_merge_action ?? "archive"}
        onValueChange={(v) => updateSettings({ post_merge_action: v } as Partial<AppConfig>)}
      />
      <p class="text-xs text-t3">Action taken automatically 30s after a PR is merged if you don't interact with the prompt.</p>
    </section>

    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Projects Base Path</h2>
      <div class="flex gap-2">
        <Input
          value={config.projects_base_path ?? ""}
          onchange={(e) => updateSettings({ projects_base_path: e.currentTarget.value || null } as Partial<AppConfig>)}
          placeholder="e.g. ~/Developer"
          class="flex-1 font-mono"
        />
        <Button type="button" onclick={pickProjectsBasePath}>Browse</Button>
      </div>
      <p class="text-xs text-t3">Default directory for the project file picker and path pre-fill.</p>
    </section>

    <!-- CLI -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Worktree Cleanup</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Remove stale worktrees</p>
          <p class="text-xs text-t3">Deletes worktree directories for sessions inactive &gt;48h.</p>
        </div>
        <Button type="button" onclick={triggerCleanupPreview}>Clean up</Button>
      </div>
      {#if cleanupMessage}
        <p class="text-xs text-t2">{cleanupMessage}</p>
      {/if}
    </section>

    <!-- CLI -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">CLI</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Install <code class="text-xs bg-panel-hi  px-1 rounded">planeai-cli</code> in PATH</p>
          <p class="text-xs text-t3">Creates a symlink at /usr/local/bin/planeai-cli</p>
        </div>
        {#if cliInstalled}
          <Button type="button" onclick={installCli}>Reinstall</Button>
        {:else}
          <Button type="button" onclick={installCli}>Install</Button>
        {/if}
      </div>
    </section>

    <!-- Logs -->
    <section class="space-y-3">
      <h2 class="text-[11px] font-semibold text-t3 uppercase tracking-[.05em]">Logs</h2>
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-t1">Application logs</p>
          <p class="text-xs text-t3">Open the folder containing log files for debugging.</p>
        </div>
        <Button type="button" onclick={openLogFolder}>Open Logs Folder</Button>
      </div>
    </section>
    {/if}
  </div>
  </div>
</div>

<Dialog open={showCleanupDialog} onOpenChange={(v) => { showCleanupDialog = v; }} title="Clean up worktrees" class="w-[480px] p-6 space-y-4">
  <h2 class="text-sm font-semibold text-t1">The following worktrees will be removed:</h2>
  <ul class="space-y-2 max-h-60 overflow-y-auto">
    {#each staleWorktrees as wt (wt.worktree_path)}
      <li class="text-xs border border-border rounded p-2">
        <p class="font-medium text-t1">{wt.session_name || "(unnamed session)"}</p>
        <p class="text-t3 font-mono">{wt.worktree_path}</p>
        {#if wt.branch}<p class="text-t3">branch: {wt.branch}</p>{/if}
      </li>
    {/each}
  </ul>
  <p class="text-xs text-amber-600">⚠ This will delete the worktree directories. This cannot be undone.</p>
  <div class="flex justify-end gap-2 pt-2">
    <Button type="button" onclick={() => { showCleanupDialog = false; }}>Cancel</Button>
    <button
      class="px-4 py-2 rounded-md text-sm font-medium bg-red-600 text-white hover:bg-red-700"
      onclick={confirmCleanup}
    >Remove {staleWorktrees.length} worktree{staleWorktrees.length === 1 ? '' : 's'}</button>
  </div>
</Dialog>
