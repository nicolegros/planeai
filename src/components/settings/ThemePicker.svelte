<script lang="ts">
  import { preferences, type TerminalSchemeSummary } from "../../lib/api";
  import { saveSettings } from "../../lib/save-settings";
  import { getSettings } from "../../lib/settings.svelte";
  import { parseSourceKey, sharedCssTheme, sideSource, sourceKey, withSideSource, type ThemeSide } from "../../lib/theme-choice";
  import Select from "../ui/Select.svelte";

  const config = $derived(getSettings());
  const choice = $derived(config.appearance.theme);
  const cssTheme = $derived(sharedCssTheme(choice));

  let available = $state<string[]>([]);
  let schemes = $state<TerminalSchemeSummary[]>([]);
  // A theme file can disappear while selected; keep it listed so the choice stays visible.
  const themes = $derived(cssTheme === null || available.includes(cssTheme) ? available : [cssTheme, ...available]);

  function cssLabel(name: string): string {
    return name.charAt(0).toUpperCase() + name.slice(1);
  }

  function items(side: ThemeSide) {
    const current = sideSource(choice, side);
    const css = typeof current === "string" || available.includes(current.css) ? available : [current.css, ...available];
    const schemeNames = schemes.filter((s) => s.dark === (side === "dark")).map((s) => s.name);
    const names = typeof current !== "string" || schemeNames.includes(current) ? schemeNames : [current, ...schemeNames];
    return [
      ...css.map((name) => ({ value: sourceKey({ css: name }), label: cssLabel(name), badge: "planeai" })),
      ...names.map((name) => ({ value: sourceKey(name), label: name })),
    ];
  }

  $effect(() => {
    preferences
      .listThemes()
      .then((names) => (available = names))
      .catch((error) => console.warn("Failed to list themes:", error));
    preferences
      .listTerminalSchemes()
      .then((list) => (schemes = list))
      .catch((error) => console.warn("Failed to list color schemes:", error));
  });

  function pick(side: ThemeSide, key: string) {
    if (!key) return;
    const theme = withSideSource(choice, side, parseSourceKey(key));
    void saveSettings({ appearance: { ...config.appearance, theme } });
  }
</script>

{#snippet sideSelect(side: ThemeSide, label: string)}
  <div class="min-w-0 flex-1 space-y-1">
    <div class="text-[12px] text-t3">{label}</div>
    <Select
      items={items(side)}
      value={sourceKey(sideSource(choice, side))}
      emptyText="No matching themes"
      ariaLabel="{label} color scheme"
      onValueChange={(key) => pick(side, key)}
    />
  </div>
{/snippet}

<div class="space-y-3">
  <div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Theme">
    {#each themes as theme (theme)}
      <button
        type="button"
        role="radio"
        aria-checked={cssTheme === theme}
        class="rounded-md px-3 py-1 text-[12px] font-medium capitalize transition-colors {cssTheme === theme ? 'bg-accent text-on-accent' : 'bg-panel-hi text-t2 hover:text-t1'}"
        onclick={() => saveSettings({ appearance: { ...config.appearance, theme } })}
      >{theme}</button>
    {/each}
  </div>
  <div class="flex gap-3">
    {@render sideSelect("light", "Light")}
    {@render sideSelect("dark", "Dark")}
  </div>
</div>
