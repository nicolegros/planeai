<script lang="ts">
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    tabs: Tab[];
    activeTabIndex: number;
    onSelect: (index: number) => void;
    onClose: (index: number) => void;
    onAdd: () => void;
  }

  let { tabs, activeTabIndex, onSelect, onClose, onAdd }: Props = $props();
</script>

<div class="flex items-center gap-0.5 w-full" role="tablist">
  {#each tabs as tab (tab.index)}
    <button
      role="tab"
      aria-selected={tab.index === activeTabIndex}
      class="flex-1 relative flex items-center justify-center px-3 h-6 rounded text-xs select-none transition-colors group
        {tab.index === activeTabIndex
          ? 'bg-white dark:bg-surface-700 text-surface-900 dark:text-surface-50 shadow-sm'
          : 'text-surface-600 dark:text-surface-400 hover:bg-surface-200/50 dark:hover:bg-surface-700/50'}"
      onclick={() => onSelect(tab.index)}
    >
      {#if tab.index !== 0}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <span
          class="absolute left-1 w-4 h-4 flex items-center justify-center rounded hover:bg-surface-300 dark:hover:bg-surface-600 text-[10px] opacity-0 group-hover:opacity-100 transition-opacity"
          role="button"
          tabindex="-1"
          aria-label="Close {tab.label}"
          onclick={(e: MouseEvent) => { e.stopPropagation(); onClose(tab.index); }}
        >×</span>
      {/if}
      <span>{tab.label}</span>
    </button>
  {/each}
  <button
    aria-label="New tab"
    class="flex items-center justify-center w-6 h-6 rounded text-xs text-surface-600 dark:text-surface-400 hover:bg-surface-200/50 dark:hover:bg-surface-700/50 transition-colors"
    onclick={onAdd}
  >+</button>
</div>
