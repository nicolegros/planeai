<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  interface Props extends HTMLInputAttributes {
    label?: string;
    checked?: boolean;
  }

  let { label, id, class: className = "", checked = $bindable(false), ...rest }: Props = $props();
</script>

<div class="flex items-center gap-2 {className}">
  <input type="checkbox" {id} class="rounded border-border accent-accent focus:ring-accent" bind:checked {...rest} />
  {#if label}
    <label for={id} class="text-sm font-medium text-t2">{label}</label>
  {/if}
</div>
