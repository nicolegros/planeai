<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLLabelAttributes } from "svelte/elements";

  interface Props extends HTMLLabelAttributes {
    children: Snippet;
  }

  let { children, class: className = "", ...rest }: Props = $props();
</script>

<label class="text-sm font-medium text-surface-700 dark:text-surface-300 {className}" {...rest}>
  {@render children()}
</label>
