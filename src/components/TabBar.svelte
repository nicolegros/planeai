<script lang="ts">
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    tabs: Tab[];
    activeTabIndex: number;
    onSelect: (index: number) => void;
    onClose: (index: number) => void;
  }

  let { tabs, activeTabIndex, onSelect, onClose }: Props = $props();
</script>

{#if tabs.length > 1}
  <div class="flex items-center h-8 bg-surface-100 dark:bg-surface-900 border-b border-surface-200 dark:border-surface-800 px-2 gap-0.5 shrink-0" role="tablist">
    {#each tabs as tab (tab.index)}
      <button
        role="tab"
        aria-selected={tab.index === activeTabIndex}
        class="flex items-center gap-1 px-3 h-6 rounded text-xs select-none transition-colors
          {tab.index === activeTabIndex
            ? 'bg-surface-200 dark:bg-surface-700 text-surface-900 dark:text-surface-50'
            : 'text-surface-600 dark:text-surface-400 hover:bg-surface-200/50 dark:hover:bg-surface-700/50'}"
        onclick={() => onSelect(tab.index)}
      >
        <span>{tab.label}</span>
        {#if tab.index !== 0}
          <span
            class="ml-1 w-4 h-4 flex items-center justify-center rounded hover:bg-surface-300 dark:hover:bg-surface-600 text-[10px]"
            role="button"
            tabindex="-1"
            aria-label="Close {tab.label}"
            onclick={(e: MouseEvent) => { e.stopPropagation(); onClose(tab.index); }}
          >×</span>
        {/if}
      </button>
    {/each}
  </div>
{/if}
