<script lang="ts">
  import type { Tab } from "../lib/session-tabs.svelte";
  import { Bot, GitCompareArrows, FileText, Terminal } from "@lucide/svelte";

  interface Props {
    tabs: Tab[];
    activeTabIndex: number;
    onSelect: (index: number) => void;
    onClose: (index: number) => void;
    onAdd: () => void;
  }

  let { tabs, activeTabIndex, onSelect, onClose, onAdd }: Props = $props();

  const icons: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompareArrows, file: FileText, terminal: Terminal };
</script>

<div class="flex items-center gap-0 h-full" role="tablist">
  {#each tabs as tab (tab.index)}
    <button
      role="tab"
      aria-selected={tab.index === activeTabIndex}
      class="relative flex items-center gap-1.5 px-3 h-full text-[12.5px] select-none transition-colors group
        {tab.index === activeTabIndex
          ? 'text-text-1'
          : 'text-text-3 hover:text-text-2'}"
      onclick={() => onSelect(tab.index)}
    >
      {#if tab.index !== 0}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <span
          class="absolute left-0.5 top-1/2 -translate-y-1/2 w-4 h-4 flex items-center justify-center rounded hover:bg-surface-200 dark:hover:bg-surface-600 text-[10px] opacity-0 group-hover:opacity-100 transition-opacity"
          role="button"
          tabindex="-1"
          aria-label="Close {tab.label}"
          onclick={(e: MouseEvent) => { e.stopPropagation(); onClose(tab.index); }}
        >×</span>
      {/if}
      {#if tab.icon && icons[tab.icon]}
        {@const Icon = icons[tab.icon]}
        <Icon size={12} class={tab.index === activeTabIndex ? 'text-primary-500' : ''} />
      {:else}
        <Terminal size={12} class={tab.index === activeTabIndex ? 'text-primary-500' : ''} />
      {/if}
      <span class="truncate font-medium">{tab.label}</span>
      {#if tab.modified}<span class="w-1.5 h-1.5 rounded-full bg-warning-400 shrink-0"></span>{/if}
      {#if tab.index === activeTabIndex}
        <span class="absolute bottom-0 left-2 right-2 h-0.5 bg-primary-500 rounded-full"></span>
      {/if}
    </button>
  {/each}
  <button
    aria-label="New tab"
    class="flex items-center justify-center w-7 h-full text-xs text-text-3 hover:text-text-2 transition-colors"
    onclick={onAdd}
  >+</button>
</div>
