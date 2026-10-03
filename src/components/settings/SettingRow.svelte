<script lang="ts">
  import type { Snippet } from "svelte";
  import { RotateCcw } from "@lucide/svelte";
  import { getSettings } from "../../lib/settings.svelte";
  import { isModified, resetPatch, settingById } from "../../lib/settings-registry";
  import SettingAnchor from "./SettingAnchor.svelte";
  import { settingsWindow } from "./settings-window.svelte";
  import { saveSettings } from "../../lib/save-settings";

  interface Props {
    /** Registry id; supplies the label, description and anchor. */
    id: string;
    /** Control below the label instead of beside it, for wide controls. */
    stacked?: boolean;
    disabled?: boolean;
    badge?: string;
    /** Extra content under the description, such as a warning. */
    note?: Snippet;
    children: Snippet;
  }

  let { id, stacked = false, disabled = false, badge, note, children }: Props = $props();

  const setting = $derived(settingById(id));
  const modified = $derived(setting ? isModified(setting, getSettings(), settingsWindow.defaults) : false);

  function reset() {
    if (!setting || !settingsWindow.defaults) return;
    void saveSettings(resetPatch(setting, getSettings(), settingsWindow.defaults));
  }
</script>

{#snippet resetButton()}
  {#if modified}
    <button
      type="button"
      class="grid h-6 w-6 shrink-0 place-items-center rounded text-t3 hover:bg-panel-hi hover:text-t1"
      title="Reset to default"
      aria-label="Reset {setting?.label} to default"
      onclick={reset}
    ><RotateCcw size={12} /></button>
  {/if}
{/snippet}

<SettingAnchor {id} class="px-4 py-3 {stacked ? 'space-y-2.5' : 'flex items-center justify-between gap-6'}">
  <div class="min-w-0 {stacked ? 'flex items-start justify-between gap-4' : ''}">
    <div class="min-w-0" class:opacity-50={disabled}>
      <div class="flex items-center gap-2 text-[13px] text-t1">
        {setting?.label ?? id}
        {#if badge}<span class="rounded bg-accent-bg px-1.5 py-px text-[10px] font-medium text-t2">{badge}</span>{/if}
      </div>
      {#if setting?.description}<p class="mt-0.5 text-[12px] leading-snug text-t3">{setting.description}</p>{/if}
      {#if note}{@render note()}{/if}
    </div>
    {#if stacked}{@render resetButton()}{/if}
  </div>
  {#if stacked}
    <div class:pointer-events-none={disabled}>{@render children()}</div>
  {:else}
    <div class="flex shrink-0 items-center gap-2">
      {@render resetButton()}
      <div class:pointer-events-none={disabled}>{@render children()}</div>
    </div>
  {/if}
</SettingAnchor>
