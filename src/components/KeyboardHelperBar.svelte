<script lang="ts">
  import { getActiveZone } from "../lib/focus.svelte";

  const TERMINAL_HINTS = [
    { k: "⌘K", l: "Command" },
    { k: "⌘N", l: "New" },
    { k: "⌘B", l: "Sidebar" },
    { k: "⌃⇥", l: "Switch" },
    { k: "⌘1–9", l: "Jump" },
    { k: "⌘\\", l: "Diff" },
  ];

  const SIDEBAR_HINTS = [
    { k: "↑↓", l: "Navigate" },
    { k: "↵", l: "Open" },
    { k: "⌘N", l: "New" },
    { k: "r", l: "Rename" },
    { k: "⌫", l: "Archive" },
    { k: "⌘B", l: "Hide" },
  ];

  let hints = $derived(getActiveZone() === "sidebar" ? SIDEBAR_HINTS : TERMINAL_HINTS);
</script>

<div class="flex items-center gap-[18px] h-[34px] px-4 border-t border-border bg-chrome shrink-0">
  {#each hints as hint (hint.k)}
    <span class="flex items-center gap-[7px] text-[11px] text-t3">
      <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel-hi">{hint.k}</span>{hint.l}
    </span>
  {/each}
</div>
