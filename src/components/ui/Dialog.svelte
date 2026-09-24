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
  }

  let { open, onOpenChange, title, description, children, class: className = "", preventEscapeClose = false, preventOpenAutoFocus = false }: Props = $props();

  const contentId = $props.id();

  /**
   * Forms driven by the form-keyboard controller start in NORMAL mode with their
   * `[data-form-keyboard]` wrapper focused. bits-ui's focus scope would otherwise
   * grab focus on the next animation frame and land on the first tabbable field,
   * which breaks the mnemonic keys and moves `document.activeElement` outside the
   * wrapper — so the app's Escape router stops yielding and Escape closes the
   * whole dialog instead of returning to NORMAL mode.
   */
  function claimFormKeyboardFocus(e: Event): void {
    const wrapper = document
      .getElementById(contentId)
      ?.querySelector<HTMLElement>("[data-form-keyboard]");
    if (!wrapper) return;
    e.preventDefault();
    wrapper.focus();
  }
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Content
      class="fixed left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2 max-h-[85vh] flex flex-col overflow-hidden outline-none rounded-lg border border-border bg-panel shadow-lg {className}"
      id={contentId}
      onEscapeKeydown={(e) => { if (preventEscapeClose) e.preventDefault(); }}
      onOpenAutoFocus={(e) => { if (preventOpenAutoFocus) { e.preventDefault(); return; } claimFormKeyboardFocus(e); }}
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
