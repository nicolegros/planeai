<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";

  interface Props extends HTMLInputAttributes {
    value?: string;
    mono?: boolean;
    /** Width utility; settings rows size fields explicitly instead of stretching. */
    width?: string;
  }

  let { value = $bindable(""), mono = false, width = "w-56", ...rest }: Props = $props();
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
  class="h-7 rounded-md border border-border-s bg-panel px-2 text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent {width} {mono ? 'font-mono text-[12px]' : 'text-[13px]'}"
  {...rest}
/>
