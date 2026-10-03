<script lang="ts">
  import { Minus, Plus } from "@lucide/svelte";
  import { preferences } from "../../lib/api";
  import { getSettings } from "../../lib/settings.svelte";
  import { Select, Switch } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import TextField from "./TextField.svelte";
  import { saveSettings } from "../../lib/save-settings";
  import { DEFAULT_SCROLLBACK_LINES, parseScrollbackLines } from "../../lib/terminal-scrollback";

  const MIN_FONT_SIZE = 8;
  const MAX_FONT_SIZE = 32;
  const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  const config = $derived(getSettings());
  let fonts = $state<{ value: string; label: string }[]>([]);
  const fontItems = $derived(
    fonts.length > 0 ? fonts : [{ value: config.terminal.font_family, label: config.terminal.font_family }],
  );

  $effect(() => {
    preferences
      .listMonospaceFonts()
      .then((names) => (fonts = names.map((name) => ({ value: name, label: name }))))
      .catch((error) => console.warn("Failed to list monospace fonts:", error));
  });

  function setFontSize(size: number) {
    if (size >= MIN_FONT_SIZE && size <= MAX_FONT_SIZE) void saveSettings({ terminal: { ...config.terminal, font_size: size } });
  }

  function setScrollback(input: HTMLInputElement) {
    const lines = parseScrollbackLines(input.value);
    // Show the clamped value even when it equals the saved one and nothing re-renders.
    input.value = String(lines ?? DEFAULT_SCROLLBACK_LINES);
    void saveSettings({ scrollback_lines: lines });
  }
</script>

<SettingsSection title="Font" help="Applies to every terminal immediately.">
  <SettingRow id="font-family" stacked>
    <div class="w-64">
      <Select
        items={fontItems}
        value={config.terminal.font_family}
        onValueChange={(font_family) => saveSettings({ terminal: { ...config.terminal, font_family } })}
        placeholder="Search fonts…"
        ariaLabel="Font family"
      />
    </div>
    <p class="mt-2 text-[12px] text-t2" style="font-family: '{config.terminal.font_family}', monospace">The quick brown fox jumps over the lazy dog</p>
  </SettingRow>
  <SettingRow id="font-size">
    <div class="inline-flex h-7 items-center rounded-md border border-border-s bg-panel">
      <button type="button" class="grid h-full w-7 place-items-center text-t2 hover:text-t1 disabled:opacity-40" aria-label="Decrease font size" disabled={config.terminal.font_size <= MIN_FONT_SIZE} onclick={() => setFontSize(config.terminal.font_size - 1)}><Minus size={12} /></button>
      <span class="min-w-10 border-x border-border px-2 text-center font-mono text-[12px] tabular-nums text-t1" aria-live="polite">{config.terminal.font_size}</span>
      <button type="button" class="grid h-full w-7 place-items-center text-t2 hover:text-t1 disabled:opacity-40" aria-label="Increase font size" disabled={config.terminal.font_size >= MAX_FONT_SIZE} onclick={() => setFontSize(config.terminal.font_size + 1)}><Plus size={12} /></button>
      <span class="pr-2 pl-1 text-[11px] text-t3">px</span>
    </div>
  </SettingRow>
</SettingsSection>

{#if IS_MAC}
  <SettingsSection title="Keyboard">
    <SettingRow id="option-as-meta">
      <Switch label="Use Option as Meta" checked={config.terminal.option_as_meta} onCheckedChange={(option_as_meta) => saveSettings({ terminal: { ...config.terminal, option_as_meta } })} />
    </SettingRow>
  </SettingsSection>
{/if}

<SettingsSection title="Behavior">
  <SettingRow id="scrollback-lines">
    <div class="flex items-center gap-2">
      <TextField
        type="number"
        min="1000"
        max="1000000"
        step="1000"
        width="w-28"
        mono
        aria-label="Scrollback lines"
        value={String(config.scrollback_lines ?? DEFAULT_SCROLLBACK_LINES)}
        onchange={(e) => setScrollback(e.currentTarget)}
      />
      <span class="text-[11px] text-t3">lines</span>
    </div>
  </SettingRow>
  <SettingRow id="web-links">
    <Switch label="Clickable links" checked={config.web_links !== false} onCheckedChange={(web_links) => saveSettings({ web_links })} />
  </SettingRow>
</SettingsSection>
