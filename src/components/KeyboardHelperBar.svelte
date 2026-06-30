<script lang="ts">
  import { getActiveZone } from "../lib/focus.svelte";
  import { getDiffTabActive } from "../lib/tab-layout.svelte";
  import { getActiveSessionId } from "../lib/session-orchestrator.svelte";
  import { MOD_LABEL } from "../lib/keyboard";

  const TERMINAL_HINTS = [
    { k: `${MOD_LABEL}K`, l: "Command" },
    { k: `${MOD_LABEL}N`, l: "New" },
    { k: `${MOD_LABEL}B`, l: "Sidebar" },
    { k: "⌃⇥", l: "Switch" },
    { k: `${MOD_LABEL}1–9`, l: "Jump" },
    { k: `${MOD_LABEL}\\`, l: "Diff" },
    { k: `${MOD_LABEL}⇧P`, l: "PR" },
  ];

  const SIDEBAR_HINTS = [
    { k: "↑↓", l: "Navigate" },
    { k: "↵", l: "Open" },
    { k: "s…", l: "Status" },
    { k: "dd", l: "Delete" },
    { k: "a", l: "Archive" },
    { k: "E", l: "Rename" },
    { k: "r", l: "Review" },
    { k: `${MOD_LABEL}N`, l: "New" },
    { k: `${MOD_LABEL}B`, l: "Hide" },
  ];

  const DIFF_HINTS = [
    { k: "j/k", l: "Navigate" },
    { k: "]/[", l: "Hunk" },
    { k: "c", l: "Comment" },
    { k: "E", l: "Edit" },
    { k: "B", l: "Compare" },
    { k: "u", l: "Unified/Split" },
    { k: "m", l: "Viewed" },
    { k: "Esc", l: "Back" },
  ];

  let isDiffActive = $derived((() => {
    const sid = getActiveSessionId();
    return sid ? (getDiffTabActive()[sid] ?? false) : false;
  })());

  let hints = $derived(isDiffActive ? DIFF_HINTS : getActiveZone() === "sidebar" ? SIDEBAR_HINTS : TERMINAL_HINTS);
</script>

<div class="flex items-center gap-[18px] h-[34px] px-4 border-t border-border bg-chrome shrink-0">
  {#each hints as hint (hint.k)}
    <span class="flex items-center gap-[7px] text-[11px] text-t3">
      <span class="font-mono text-[10px] text-t2 border border-border rounded-[5px] px-1.5 py-[2px] bg-panel-hi">{hint.k}</span>{hint.l}
    </span>
  {/each}
</div>
