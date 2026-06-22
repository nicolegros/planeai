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
  autocomplete="off"
  autocorrect="off"
  autocapitalize="off"
  spellcheck={false}
  data-form-type="other"
  data-1p-ignore
  data-lpignore="true"
  class="w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 {className}"
  {...rest}
/>
