<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  interface Props extends HTMLInputAttributes {
    value?: string;
    class?: string;
  }

  let { value = $bindable(""), class: className = "", ...rest }: Props = $props();
</script>

<input
  bind:value
  class="w-full rounded border border-surface-300 bg-surface-50 px-3 py-2 text-sm text-surface-900 placeholder:text-surface-400 dark:border-surface-600 dark:bg-surface-900 dark:text-surface-50 dark:placeholder:text-surface-500 {className}"
  {...rest}
/>
