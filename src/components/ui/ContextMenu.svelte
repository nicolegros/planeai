<script lang="ts">
  import type { Snippet } from "svelte";

  interface MenuItem {
    label: string;
    danger?: boolean;
    onSelect: () => void;
  }

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onClose: () => void;
  }

  let { x, y, items, onClose }: Props = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="fixed inset-0 z-50" onclick={onClose} oncontextmenu={(e) => { e.preventDefault(); onClose(); }}>
  <div
    class="absolute rounded border border-surface-200 bg-surface-50 shadow-lg py-1 text-sm w-40 dark:border-surface-700 dark:bg-surface-900"
    style="left: {x}px; top: {y}px;"
  >
    {#each items as item}
      <button
        class="w-full text-left px-3 py-1.5 hover:bg-surface-100 dark:hover:bg-surface-800 {item.danger ? 'text-error-600 dark:text-error-400' : 'text-surface-700 dark:text-surface-300'}"
        onclick={() => { item.onSelect(); onClose(); }}
      >
        {item.label}
      </button>
    {/each}
  </div>
</div>
