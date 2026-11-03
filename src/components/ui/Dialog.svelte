<script lang="ts">
  import { Dialog } from "bits-ui";
  import type { Snippet } from "svelte";
  import { FRAME_INTERACTION_EVENT } from "../../lib/frame-interaction";

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
    /**
     * `center` re-centers the dialog whenever its content changes height; `top` keeps its top edge still near the
     * top of the window; `center-hold` opens centered, then keeps its top edge still once the user acts in it.
     */
    anchor?: "center" | "top" | "center-hold";
  }

  let { open, onOpenChange, title, description, children, class: className = "", preventEscapeClose = false, preventOpenAutoFocus = false, initialFocusSelector, anchor = "center" }: Props = $props();

  const anchors = { center: "top-1/2 -translate-y-1/2", "center-hold": "top-1/2 -translate-y-1/2", top: "top-[7.5vh]" } as const;
  /** Matches the 85vh height cap, so a held dialog never comes closer to the window edges than a centered one. */
  const EDGE_VH = 0.075;

  let contentEl = $state<HTMLElement | null>(null);
  let heldTop = $state<number | null>(null);
  let height = $state(0);
  let viewportHeight = $state(0);

  const heldStyle = $derived.by(() => {
    if (heldTop === null) return undefined;
    const top = Math.max(viewportHeight * EDGE_VH, Math.min(heldTop, viewportHeight * (1 - EDGE_VH) - height));
    return `top: ${top}px`;
  });

  $effect(() => {
    const el = contentEl;
    if (anchor !== "center-hold" || !el) return;
    const hold = () => {
      if (heldTop === null) heldTop = el.getBoundingClientRect().top;
    };
    const observer = new ResizeObserver(() => (height = el.offsetHeight));
    observer.observe(el);
    const events = ["pointerdown", "keydown", FRAME_INTERACTION_EVENT];
    for (const type of events) el.addEventListener(type, hold, true);
    return () => {
      observer.disconnect();
      for (const type of events) el.removeEventListener(type, hold, true);
    };
  });

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

<svelte:window bind:innerHeight={viewportHeight} />

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Content
      class="fixed left-1/2 {heldTop === null ? anchors[anchor] : ''} z-50 -translate-x-1/2 max-h-[85vh] flex flex-col overflow-hidden outline-none rounded-lg border border-border bg-panel shadow-lg {className}"
      style={heldStyle}
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
