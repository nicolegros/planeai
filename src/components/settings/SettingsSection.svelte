<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    help?: string;
    /** Render children as the card itself, for sections that hold a single list. */
    bare?: boolean;
    actions?: Snippet;
    children: Snippet;
  }

  let { title, help, bare = false, actions, children }: Props = $props();
</script>

<section class="mt-7 first:mt-0">
  <div class="mb-2 ml-1 flex items-end justify-between gap-4">
    <div class="min-w-0">
      <h2 class="text-[12px] font-medium text-t1">{title}</h2>
      {#if help}<p class="mt-0.5 text-[12px] leading-snug text-t3">{help}</p>{/if}
    </div>
    {#if actions}<div class="shrink-0">{@render actions()}</div>{/if}
  </div>
  {#if bare}
    {@render children()}
  {:else}
    <div class="divide-y divide-border overflow-hidden rounded-xl border border-border-s bg-panel">
      {@render children()}
    </div>
  {/if}
</section>
