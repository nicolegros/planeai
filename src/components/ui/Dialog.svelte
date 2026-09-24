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
    preventEscapeClose?: boolean;
    preventOpenAutoFocus?: boolean;
    /**
     * Selector for the element that should receive focus when the dialog opens,
     * instead of bits-ui's focus scope choosing the first tabbable field. The
     * target must be programmatically focusable (e.g. `tabindex="-1"`).
     */
    initialFocusSelector?: string;
  }

  let { open, onOpenChange, title, description, children, class: className = "", preventEscapeClose = false, preventOpenAutoFocus = false, initialFocusSelector }: Props = $props();

  let contentEl = $state<HTMLElement | null>(null);

  /**
   * bits-ui's focus scope claims focus on the next animation frame, overriding
   * anything the content focused itself. Honour `initialFocusSelector` instead;
   * fall through when it is unset or unmatched so other dialogs keep the default.
   */
  function claimInitialFocus(e: Event): void {
    if (!initialFocusSelector) return;
    const target = contentEl?.querySelector<HTMLElement>(initialFocusSelector);
    if (!target) return;
    target.focus();
    if (document.activeElement !== target) {
      // Cancelling the default here would leave the dialog with nothing focused;
      // fall through to bits-ui's first-tabbable fallback instead.
      if (import.meta.env.DEV) {
        console.warn(`Dialog initialFocusSelector "${initialFocusSelector}" could not take focus`);
      }
      return;
    }
    e.preventDefault();
  }
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Content
      class="fixed left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2 max-h-[85vh] flex flex-col overflow-hidden outline-none rounded-lg border border-border bg-panel shadow-lg {className}"
      bind:ref={contentEl}
      onEscapeKeydown={(e) => { if (preventEscapeClose) e.preventDefault(); }}
      onOpenAutoFocus={(e) => { if (preventOpenAutoFocus) { e.preventDefault(); return; } claimInitialFocus(e); }}
      onInteractOutside={(e) => { if (preventEscapeClose) e.preventDefault(); }}
    >
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
