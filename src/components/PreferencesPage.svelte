<script lang="ts">
  import { getSettings, updateSettings, type AppearanceMode } from "../lib/settings.svelte";
  import { getThemesByVariant } from "../lib/terminal-themes";

  interface Props {
    onBack: () => void;
  }

  let { onBack }: Props = $props();

  const settings = $derived(getSettings());
  const darkThemes = getThemesByVariant("dark");
  const lightThemes = getThemesByVariant("light");

  function setAppearance(mode: AppearanceMode) {
    updateSettings({ appearance_mode: mode });
  }

  function setFontSize(size: number) {
    if (size >= 8 && size <= 32) updateSettings({ font_size: size });
  }
</script>

<div class="h-full overflow-y-auto bg-surface-50 dark:bg-surface-950 p-8">
  <div class="max-w-2xl mx-auto space-y-8">
    <div class="flex items-center gap-3">
      <button
        class="rounded p-1.5 text-surface-500 hover:text-surface-900 dark:hover:text-surface-100 hover:bg-surface-200 dark:hover:bg-surface-800"
        onclick={onBack}
        aria-label="Back"
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>
      </button>
      <h1 class="text-xl font-semibold text-surface-900 dark:text-surface-50">Preferences</h1>
    </div>

    <!-- Appearance Mode -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-400 uppercase tracking-wide">Appearance</h2>
      <div class="flex gap-2">
        {#each ["system", "light", "dark"] as mode (mode)}
          <button
            class="px-4 py-2 rounded-md text-sm font-medium capitalize transition-colors {settings.appearance_mode === mode ? 'bg-primary-500 text-white' : 'bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700'}"
            onclick={() => setAppearance(mode as AppearanceMode)}
          >{mode}</button>
        {/each}
      </div>
    </section>

    <!-- Dark Terminal Theme -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-400 uppercase tracking-wide">Dark Terminal Theme</h2>
      <div class="grid grid-cols-3 gap-3">
        {#each darkThemes as theme (theme.id)}
          <button
            class="rounded-lg border-2 p-3 text-left transition-colors {settings.terminal_theme_dark === theme.id ? 'border-primary-500' : 'border-surface-200 dark:border-surface-700 hover:border-surface-400 dark:hover:border-surface-500'}"
            onclick={() => updateSettings({ terminal_theme_dark: theme.id })}
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
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-400 uppercase tracking-wide">Light Terminal Theme</h2>
      <div class="grid grid-cols-3 gap-3">
        {#each lightThemes as theme (theme.id)}
          <button
            class="rounded-lg border-2 p-3 text-left transition-colors {settings.terminal_theme_light === theme.id ? 'border-primary-500' : 'border-surface-200 dark:border-surface-700 hover:border-surface-400 dark:hover:border-surface-500'}"
            onclick={() => updateSettings({ terminal_theme_light: theme.id })}
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

    <!-- Font Size -->
    <section class="space-y-3">
      <h2 class="text-sm font-medium text-surface-600 dark:text-surface-400 uppercase tracking-wide">Font Size</h2>
      <div class="flex items-center gap-3">
        <button
          class="rounded border border-surface-300 dark:border-surface-600 px-3 py-1.5 text-sm text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800"
          onclick={() => setFontSize(settings.font_size - 1)}
        >−</button>
        <span class="text-lg font-mono text-surface-900 dark:text-surface-50 w-8 text-center">{settings.font_size}</span>
        <button
          class="rounded border border-surface-300 dark:border-surface-600 px-3 py-1.5 text-sm text-surface-700 dark:text-surface-300 hover:bg-surface-200 dark:hover:bg-surface-800"
          onclick={() => setFontSize(settings.font_size + 1)}
        >+</button>
        <span class="text-xs text-surface-500">px (8–32)</span>
      </div>
    </section>
  </div>
</div>
