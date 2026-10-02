<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Bot, Code, ListTodo, Palette, Puzzle, Search, Server, SlidersHorizontal, SquareTerminal, Wrench, X, ArrowRight } from "@lucide/svelte";
  import type { Component } from "svelte";
  import { plugins, updater } from "../../lib/api";
  import { getSettings, loadSettings } from "../../lib/settings.svelte";
  import { loadTheme } from "../../lib/theme-loader";
  import { initUpdateListener, resetManualUpdateState, setLaunchUpdateAvailable } from "../../lib/updater.svelte";
  import {
    SETTINGS_CATEGORIES,
    categoryById,
    parseSettingsLocation,
    searchSettings,
    type PluginSettingsPage,
    type SettingsCategoryId,
    type SettingsLocation,
    type SettingsSearchResult,
  } from "../../lib/settings-registry";
  import type { PluginInventory, PluginUiContribution } from "../../lib/types";
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import AdvancedPage from "./AdvancedPage.svelte";
  import AgentsPage from "./AgentsPage.svelte";
  import AppearancePage from "./AppearancePage.svelte";
  import EditorPage from "./EditorPage.svelte";
  import GeneralPage from "./GeneralPage.svelte";
  import PluginsPage from "./PluginsPage.svelte";
  import SessionsPage from "./SessionsPage.svelte";
  import TasksPage from "./TasksPage.svelte";
  import TerminalPage from "./TerminalPage.svelte";
  import { loadDefaults, navigateSettings, settingsWindow } from "./settings-window.svelte";

  /** Below this width the sidebar collapses to icons. */
  const NARROW_WIDTH = 820;
  const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  const ICONS: Record<SettingsCategoryId, Component<{ size?: number; class?: string }>> = {
    general: SlidersHorizontal,
    appearance: Palette,
    terminal: SquareTerminal,
    agents: Bot,
    sessions: Server,
    tasks: ListTodo,
    editor: Code,
    plugins: Puzzle,
    advanced: Wrench,
  };

  let loaded = $state(false);
  let inventory = $state<PluginInventory[]>([]);
  let inventoryLoaded = $state(false);
  let windowWidth = $state(typeof window !== "undefined" ? window.innerWidth : 1024);
  let searchEl = $state<HTMLInputElement | null>(null);
  let unlistenNavigate: UnlistenFn | undefined;

  const narrow = $derived(windowWidth < NARROW_WIDTH);
  const pluginContributions = $derived(
    inventory
      .filter((plugin) => plugin.state === "running")
      .flatMap((plugin) =>
        plugin.ui_contributions
          .filter((contribution) => contribution.placement === "preferences")
          .map((contribution) => ({ plugin, contribution, key: `${plugin.id}:${contribution.id}` })),
      )
      .sort((a, b) => (a.contribution.order ?? 0) - (b.contribution.order ?? 0) || a.plugin.name.localeCompare(b.plugin.name)),
  );
  const pluginPages = $derived<PluginSettingsPage[]>(
    pluginContributions.map(({ plugin, contribution, key }) => ({ key, label: contribution.label, pluginName: plugin.name })),
  );
  const results = $derived(searchSettings(settingsWindow.query, pluginPages, getSettings()));
  const location = $derived(settingsWindow.location);
  const activePlugin = $derived(location.kind === "plugin" ? pluginContributions.find((entry) => entry.key === location.key) : undefined);
  const activeCategory = $derived(location.kind === "category" ? categoryById(location.id) : undefined);

  function isActive(target: SettingsLocation): boolean {
    if (settingsWindow.query) return false;
    if (target.kind === "plugin") return location.kind === "plugin" && location.key === target.key;
    return location.kind === "category" && location.id === target.id;
  }

  function openResult(result: SettingsSearchResult) {
    navigateSettings(result.kind === "plugin" ? { kind: "plugin", key: result.key } : { kind: "category", id: result.setting.category, settingId: result.id });
  }

  function handleKeydown(event: KeyboardEvent) {
    const mod = IS_MAC ? event.metaKey : event.ctrlKey;
    const key = event.key.toLowerCase();
    if (mod && key === "w") {
      event.preventDefault();
      event.stopPropagation();
      void getCurrentWindow().close();
    } else if (mod && key === "f") {
      event.preventDefault();
      searchEl?.focus();
      searchEl?.select();
    }
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && results[0]) {
      event.preventDefault();
      openResult(results[0]);
      searchEl?.blur();
    } else if (event.key === "Escape" && settingsWindow.query) {
      event.preventDefault();
      settingsWindow.query = "";
    }
  }

  onMount(async () => {
    window.addEventListener("keydown", handleKeydown, true);
    settingsWindow.location = parseSettingsLocation(window.location.search, window.location.hash);
    // Listen first: the main window may send a location while this window is still loading.
    unlistenNavigate = await listen<SettingsLocation>("preferences-navigate", (event) => navigateSettings(event.payload));
    await initUpdateListener();
    try {
      const update = await updater.getPending();
      if (update) setLaunchUpdateAvailable(update);
    } catch (error) {
      console.warn("Failed to load pending update:", error);
    }
    await loadSettings();
    loadTheme();
    loaded = true;
    void loadDefaults();
    plugins
      .list()
      .then((list) => (inventory = list))
      .catch((error) => console.warn("Failed to list plugins:", error))
      .finally(() => (inventoryLoaded = true));
    // Reveal the target now that its page can render, whether it came from the URL or an early event.
    if (settingsWindow.location.kind === "category" && settingsWindow.location.settingId) navigateSettings(settingsWindow.location);
  });

  onDestroy(() => {
    window.removeEventListener("keydown", handleKeydown, true);
    unlistenNavigate?.();
    resetManualUpdateState();
  });
</script>

<svelte:window bind:innerWidth={windowWidth} />

{#snippet searchField()}
  <label class="flex h-7 items-center gap-1.5 rounded-md bg-panel-hi px-2 text-t3 focus-within:ring-1 focus-within:ring-accent">
    <Search size={13} class="shrink-0" />
    <input
      bind:this={searchEl}
      bind:value={settingsWindow.query}
      onkeydown={handleSearchKeydown}
      placeholder="Search settings"
      aria-label="Search settings"
      autocomplete="off"
      spellcheck={false}
      class="w-full min-w-0 bg-transparent text-[13px] text-t1 placeholder:text-t3 focus:outline-none"
    />
    {#if settingsWindow.query}
      <button type="button" class="shrink-0 hover:text-t1" aria-label="Clear search" onclick={() => {
        settingsWindow.query = "";
      }}><X size={12} /></button>
    {:else}
      <kbd class="shrink-0 font-mono text-[10px]">{IS_MAC ? "⌘F" : "Ctrl F"}</kbd>
    {/if}
  </label>
{/snippet}

{#snippet navItem(target: SettingsLocation, label: string, Icon: Component<{ size?: number; class?: string }>, nested: boolean)}
  {@const active = isActive(target)}
  <button
    type="button"
    class="flex items-center gap-2.5 rounded-md py-[5px] text-left text-[13px] transition-colors {narrow ? 'justify-center px-0' : nested ? 'pl-5 pr-2' : 'px-2'} {active ? 'bg-accent-bg text-t1' : 'text-t2 hover:bg-panel-hi/60 hover:text-t1'}"
    title={narrow ? label : undefined}
    aria-current={active ? "page" : undefined}
    onclick={() => navigateSettings(target)}
  >
    <span class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] {active ? 'bg-accent text-on-accent' : 'bg-panel-hi text-t2'}"><Icon size={12} /></span>
    {#if !narrow}<span class="truncate">{label}</span>{:else}<span class="sr-only">{label}</span>{/if}
  </button>
{/snippet}

<div class="flex h-screen overflow-hidden bg-canvas text-t1">
  <aside class="flex shrink-0 flex-col gap-3 border-r border-border bg-sidebar px-2.5 pb-3 pt-3 {narrow ? 'w-[52px]' : 'w-[212px]'}" aria-label="Settings categories">
    {#if !narrow}{@render searchField()}{/if}
    <nav class="flex flex-1 flex-col gap-px overflow-y-auto">
      {#each SETTINGS_CATEGORIES as category (category.id)}
        {#if category.id === "advanced"}<div class="my-1.5 h-px shrink-0 bg-border"></div>{/if}
        {@render navItem({ kind: "category", id: category.id }, category.label, ICONS[category.id], false)}
        {#if category.id === "plugins"}
          {#each pluginContributions as entry (entry.key)}
            {@render navItem({ kind: "plugin", key: entry.key }, entry.contribution.label, Puzzle, true)}
          {/each}
        {/if}
      {/each}
    </nav>
  </aside>

  <main class="min-w-0 flex-1 overflow-y-auto bg-main">
    <div class="mx-auto max-w-[620px] px-8 pb-16 pt-8">
      {#if narrow}<div class="mb-6">{@render searchField()}</div>{/if}
      {#if !loaded}
        <p class="text-[13px] text-t3">Loading settings…</p>
      {:else if settingsWindow.query}
        <h1 class="text-[20px] font-semibold tracking-tight">Results for “{settingsWindow.query.trim()}”</h1>
        <p class="mt-1 text-[12px] text-t3">
          {results.length} result{results.length === 1 ? "" : "s"}{results.length ? " · Enter opens the first one" : ""}
        </p>
        {#if results.length}
          <ul class="mt-5 divide-y divide-border overflow-hidden rounded-xl border border-border-s bg-panel" aria-label="Search results">
            {#each results as result (result.id)}
              <li>
                <button type="button" class="group flex w-full items-center gap-4 px-4 py-3 text-left hover:bg-panel-hi/50" onclick={() => openResult(result)}>
                  <div class="min-w-0 flex-1">
                    {#if result.kind === "setting"}
                      <div class="text-[11px] text-t3">{categoryById(result.setting.category)?.label} › {result.setting.section}</div>
                      <div class="text-[13px] text-t1">{result.setting.label}</div>
                      {#if result.setting.description}<div class="mt-0.5 text-[12px] leading-snug text-t3">{result.setting.description}</div>{/if}
                    {:else}
                      <div class="text-[11px] text-t3">Plugins › {result.pluginName}</div>
                      <div class="text-[13px] text-t1">{result.label}</div>
                    {/if}
                  </div>
                  <ArrowRight size={13} class="shrink-0 text-t3 opacity-0 transition-opacity group-hover:opacity-100" />
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="mt-16 text-center text-[13px] text-t3">No settings match. Try “font”, “tmux” or “vim”.</p>
        {/if}
      {:else if activePlugin}
        <header class="mb-7 flex items-center gap-3">
          <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-accent text-on-accent"><Puzzle size={18} /></span>
          <div class="min-w-0">
            <h1 class="text-[20px] font-semibold leading-tight tracking-tight">{activePlugin.contribution.label}</h1>
            <p class="text-[12px] text-t3">Provided by the {activePlugin.plugin.name} plugin.</p>
          </div>
        </header>
        {#key activePlugin.key}
          <div data-plugin-preference-contribution={activePlugin.key}>
            <PluginContributionHost plugin={activePlugin.plugin} contribution={activePlugin.contribution as PluginUiContribution} onNavigate={() => {}} onClose={() => {}} />
          </div>
        {/key}
      {:else if location.kind === "plugin"}
        <p class="text-[13px] text-t3">
          {inventoryLoaded ? "This plugin page is not available. The plugin may be disabled or still starting." : "Loading plugin…"}
        </p>
      {:else if activeCategory}
        {@const Icon = ICONS[activeCategory.id]}
        <header class="mb-7 flex items-center gap-3">
          <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-accent text-on-accent"><Icon size={18} /></span>
          <div class="min-w-0">
            <h1 class="text-[20px] font-semibold leading-tight tracking-tight">{activeCategory.label}</h1>
            <p class="text-[12px] text-t3">{activeCategory.description}</p>
          </div>
        </header>
        {#key activeCategory.id}
          {#if activeCategory.id === "general"}<GeneralPage />
          {:else if activeCategory.id === "appearance"}<AppearancePage />
          {:else if activeCategory.id === "terminal"}<TerminalPage />
          {:else if activeCategory.id === "agents"}<AgentsPage />
          {:else if activeCategory.id === "sessions"}<SessionsPage />
          {:else if activeCategory.id === "tasks"}<TasksPage />
          {:else if activeCategory.id === "editor"}<EditorPage />
          {:else if activeCategory.id === "plugins"}<PluginsPage onInventoryChange={(next) => (inventory = next)} />
          {:else if activeCategory.id === "advanced"}<AdvancedPage />
          {/if}
        {/key}
      {/if}
    </div>
  </main>
</div>
