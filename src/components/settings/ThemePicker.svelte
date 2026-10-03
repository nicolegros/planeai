<script lang="ts">
  import { preferences } from "../../lib/api";
  import { saveSettings } from "../../lib/save-settings";
  import { getSettings } from "../../lib/settings.svelte";

  const config = $derived(getSettings());
  let available = $state<string[]>([]);
  // A theme file can disappear while selected; keep it listed so the choice stays visible.
  const themes = $derived(available.includes(config.appearance.theme) ? available : [config.appearance.theme, ...available]);

  $effect(() => {
    preferences
      .listThemes()
      .then((names) => (available = names))
      .catch((error) => console.warn("Failed to list themes:", error));
  });
</script>

<div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Theme">
  {#each themes as theme (theme)}
    <button
      type="button"
      role="radio"
      aria-checked={config.appearance.theme === theme}
      class="rounded-md px-3 py-1 text-[12px] font-medium capitalize transition-colors {config.appearance.theme === theme ? 'bg-accent text-on-accent' : 'bg-panel-hi text-t2 hover:text-t1'}"
      onclick={() => saveSettings({ appearance: { ...config.appearance, theme } })}
    >{theme}</button>
  {/each}
</div>
