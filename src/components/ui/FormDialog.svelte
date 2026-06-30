<script lang="ts">
  import type { Snippet } from "svelte";
  import Dialog from "./Dialog.svelte";

  interface Props {
    title: string;
    onClose: () => void;
    children: Snippet;
    preventAutoFocus?: boolean;
  }

  let { title, onClose, children, preventAutoFocus = false }: Props = $props();
</script>

<Dialog open={true} onOpenChange={(v) => { if (!v) onClose(); }} {title} class="w-[452px] rounded-xl border-border-s shadow-[0_26px_70px_-14px_rgba(0,0,0,0.6)] overflow-hidden" preventEscapeClose {preventAutoFocus}>
  <div class="px-5 pt-5 pb-3 text-[15px] font-semibold text-t1">{title}</div>
  {@render children()}
</Dialog>
