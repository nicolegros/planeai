<script lang="ts">
  interface Props {
    value: string;
    onCommit: () => void;
    onCancel: () => void;
  }

  let { value = $bindable(), onCommit, onCancel }: Props = $props();

  function autofocus(node: HTMLInputElement) { requestAnimationFrame(() => node.focus()); }
</script>

<input
  use:autofocus
  class="w-full px-2 py-1.5 rounded-md text-sm bg-panel border border-accent outline-none text-t1"
  aria-label="Rename session"
  bind:value
  onkeydown={(e) => { if (e.key === "Enter") onCommit(); if (e.key === "Escape") onCancel(); }}
  onblur={onCommit}
/>
