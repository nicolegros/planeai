<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  interface Props extends HTMLInputAttributes {
    label?: string;
    checked?: boolean;
  }

  let { label, id, class: className = "", checked = $bindable(false), ...rest }: Props = $props();
</script>

<div class="flex items-center gap-2 {className}">
  <input type="checkbox" {id} class="rounded border-surface-300 dark:border-surface-600" bind:checked {...rest} />
  {#if label}
    <label for={id} class="text-sm font-medium text-surface-700 dark:text-surface-300">{label}</label>
  {/if}
</div>
