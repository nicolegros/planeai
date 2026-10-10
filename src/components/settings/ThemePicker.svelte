<script lang="ts">
  import { preferences, type TerminalSchemeSummary } from "../../lib/api";
  import { saveSettings } from "../../lib/save-settings";
  import { getSettings } from "../../lib/settings.svelte";
  import Select from "../ui/Select.svelte";

  type Side = "light" | "dark";

  const DEFAULT_SCHEMES = { light: "One Half Light", dark: "One Half Dark" };

  const config = $derived(getSettings());
  const choice = $derived(config.appearance.theme);
  const cssTheme = $derived(typeof choice === "string" ? choice : null);
  const schemes = $derived(typeof choice === "string" ? null : choice);

  let available = $state<string[]>([]);
  let terminalSchemes = $state<TerminalSchemeSummary[]>([]);
  // A theme file can disappear while selected; keep it listed so the choice stays visible.
  const themes = $derived(cssTheme === null || available.includes(cssTheme) ? available : [cssTheme, ...available]);
  const schemeItems = $derived({
    light: terminalSchemes.filter((s) => !s.dark).map((s) => ({ value: s.name, label: s.name })),
    dark: terminalSchemes.filter((s) => s.dark).map((s) => ({ value: s.name, label: s.name })),
  });

  $effect(() => {
    preferences
      .listThemes()
      .then((names) => (available = names))
      .catch((error) => console.warn("Failed to list themes:", error));
    preferences
      .listTerminalSchemes()
      .then((list) => (terminalSchemes = list))
      .catch((error) => console.warn("Failed to list Ghostty themes:", error));
  });

  function pickScheme(side: Side, name: string) {
    if (!name) return;
    const theme = { ...(schemes ?? DEFAULT_SCHEMES), [side]: name };
    void saveSettings({ appearance: { ...config.appearance, theme } });
  }
</script>

{#snippet schemeSelect(side: Side, label: string)}
  <div class="min-w-0 flex-1 space-y-1">
    <div class="text-[12px] text-t3">{label}</div>
    <Select
      items={schemeItems[side]}
      value={schemes?.[side] ?? ""}
      placeholder={DEFAULT_SCHEMES[side]}
      emptyText="No matching themes"
      ariaLabel="{label} Ghostty theme"
      onValueChange={(name) => pickScheme(side, name)}
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
  <div class="space-y-1.5">
    <div class="text-[12px] font-medium text-t2">Ghostty themes</div>
    <div class="flex gap-3">
      {@render schemeSelect("light", "Light")}
      {@render schemeSelect("dark", "Dark")}
    </div>
  </div>
</div>
