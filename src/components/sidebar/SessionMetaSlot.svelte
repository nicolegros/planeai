<script lang="ts">
  import { Tooltip } from "bits-ui";
  import PluginContributionHost from "../PluginContributionHost.svelte";
  import { metaSlotPanelOf } from "../../lib/plugin-navigation";
  import { PLUGIN_TONE_COLORS, type PluginTooltipLine } from "../../lib/plugin-tooltip";
  import type { PluginSessionContext } from "../../lib/plugin-sdk";
  import type { PluginSlot } from "../../lib/types";

  interface Props {
    item: PluginSlot;
    session: PluginSessionContext;
    onNavigate: (pluginId: string, contributionId: string) => void;
    onClose: () => void;
    onOpenPanel?: (sessionId: string, pluginId: string, contributionId: string) => void;
  }

  let { item, session, onNavigate, onClose, onOpenPanel }: Props = $props();

  let lines = $state<PluginTooltipLine[]>([]);
  const panel = $derived(onOpenPanel ? metaSlotPanelOf(item.plugin) : undefined);
  const label = $derived([item.contribution.label, ...lines.map((line) => line.text)].join(", "));
</script>

{#snippet frame()}
  <PluginContributionHost plugin={item.plugin} contribution={item.contribution} {session} {onNavigate} {onClose} onTooltip={(next) => (lines = next)} />
{/snippet}

{#if panel && onOpenPanel}
  <Tooltip.Root disabled={lines.length === 0}>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <!-- Hidden until the plugin reports content, so an empty slot is never a tab stop. -->
        <button
          {...props}
          type="button"
          class="flex shrink-0 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-accent [&:not(:has([data-plugin-indicator-visible=true]))]:invisible"
          aria-label={label}
          onclick={(e) => { e.stopPropagation(); (props.onclick as ((event: MouseEvent) => void) | undefined)?.(e); onOpenPanel(session.id, item.plugin.id, panel.id); }}
        >{@render frame()}</button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Portal>
      <Tooltip.Content side="bottom" align="start" sideOffset={6} class="z-50 flex max-w-80 flex-col gap-0.5 rounded-md border border-border bg-panel px-2.5 py-1.5 text-xs shadow-lg">
        {#each lines as line, index (index)}
          <span class="truncate text-t1" style:color={line.tone ? `var(${PLUGIN_TONE_COLORS[line.tone]})` : undefined}>{line.text}</span>
        {/each}
      </Tooltip.Content>
    </Tooltip.Portal>
  </Tooltip.Root>
{:else}
  <span class="flex shrink-0">{@render frame()}</span>
{/if}
