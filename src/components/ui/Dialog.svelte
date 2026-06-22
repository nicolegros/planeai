<script lang="ts">
  import { Dialog } from "bits-ui";
  import type { Snippet } from "svelte";

  interface Props {
    open: boolean;
    onOpenChange?: (open: boolean) => void;
    title?: string;
    description?: string;
    children: Snippet;
    class?: string;
  }

  let { open, onOpenChange, title, description, children, class: className = "" }: Props = $props();
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Content class="fixed left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-panel shadow-lg {className}">
      {#if title}
        <Dialog.Title class="sr-only">{title}</Dialog.Title>
      {/if}
      {#if description}
        <Dialog.Description class="sr-only">{description}</Dialog.Description>
      {/if}
      {@render children()}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
