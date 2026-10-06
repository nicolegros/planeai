<script lang="ts">
  import type { Snippet } from "svelte";
  import { X } from "@lucide/svelte";
  import Dialog from "./Dialog.svelte";

  interface Props {
    title: string;
    onClose: () => void;
    children: Snippet;
    class?: string;
    preventEscapeClose?: boolean;
    preventOpenAutoFocus?: boolean;
    /** `form` fits the host's own forms; `wide` fits plugin content. Both stay within the viewport. */
    size?: "form" | "wide";
    /** Shows a close button beside the title, for content that has no Cancel of its own. */
    closeButton?: boolean;
  }

  let { title, onClose, children, class: className = "", preventEscapeClose = true, preventOpenAutoFocus = false, size = "form", closeButton = false }: Props = $props();

  const widths = { form: "w-[min(452px,calc(100vw-32px))]", wide: "w-[min(640px,calc(100vw-32px))]" } as const;
</script>

<Dialog open={true} onOpenChange={(v) => { if (!v) onClose(); }} {title} class="{widths[size]} rounded-xl border-border-s shadow-[0_26px_70px_-14px_rgba(0,0,0,0.6)] overflow-hidden {className}" {preventEscapeClose} {preventOpenAutoFocus} initialFocusSelector="[data-form-keyboard]">
  <div class="flex flex-shrink-0 items-center justify-between gap-3 px-5 pt-5 pb-3">
    <div class="min-w-0 truncate text-[15px] font-semibold text-t1">{title}</div>
    {#if closeButton}
      <button type="button" class="-my-1 -mr-1.5 flex h-[28px] w-[28px] shrink-0 items-center justify-center rounded-[6px] text-t3 transition-colors hover:bg-panel-hi hover:text-t1" aria-label="Close {title}" onclick={onClose}>
        <X size={14} />
      </button>
    {/if}
  </div>
  <div class="flex-1 min-h-0 overflow-y-auto overscroll-contain">
    {@render children()}
  </div>
</Dialog>
