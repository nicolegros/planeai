<script lang="ts">
  import { Dialog } from "./ui";
  import { IS_MAC, MOD_LABEL, MOD_ENTER_HINT } from "../lib/keyboard";
  import { getActiveZone } from "../lib/focus.svelte";
  import { filterShortcuts } from "../lib/shortcut-filter";

  interface Props {
    open: boolean;
    onOpenChange: (open: boolean) => void;
  }

  let { open, onOpenChange }: Props = $props();
  let searchQuery = $state("");
  let searchInput: HTMLInputElement | undefined = $state();

  const shortcuts = [
    { section: "General", items: [
      { keys: `${MOD_LABEL}K`, description: "Command menu" },
      { keys: `${MOD_LABEL}/`, description: "Keyboard shortcuts" },
      { keys: `${MOD_LABEL},`, description: "Preferences" },
      { keys: `Escape`, description: "Dismiss / focus terminal" },
    ]},
    { section: "Sessions", items: [
      { keys: `${MOD_LABEL}N`, description: "New session" },
      { keys: `${MOD_LABEL}⇧N`, description: "New project" },
      { keys: `${MOD_LABEL}1–9`, description: "Jump to session" },
      { keys: `Ctrl+Tab`, description: "Switch session (MRU)" },
      { keys: `${MOD_LABEL}}`, description: "Next session" },
      { keys: `${MOD_LABEL}{`, description: "Previous session" },
    ]},
    { section: "Tabs", items: [
      { keys: `${MOD_LABEL}T`, description: "New tab" },
      { keys: `${MOD_LABEL}W`, description: "Close tab" },
      { keys: `${MOD_LABEL}]`, description: "Next tab" },
      { keys: `${MOD_LABEL}[`, description: "Previous tab" },
    ]},
    { section: "View", items: [
      { keys: `${MOD_LABEL}B`, description: "Toggle sidebar" },
      { keys: `${MOD_LABEL}⇧S`, description: "Focus sessions panel (jumps to active)" },
      { keys: `${MOD_LABEL}⇧T`, description: "Focus tasks panel" },
      { keys: `${MOD_LABEL}R`, description: "Refresh tasks" },
      { keys: `${MOD_LABEL}⇧R`, description: "Open Review" },
      { keys: `${MOD_LABEL}E`, description: "Toggle file explorer" },
      { keys: `${MOD_LABEL}\\`, description: "Toggle diff" },
    ]},
    { section: "Splits", items: [
      { keys: `${MOD_LABEL}D`, description: "Split vertical" },
      { keys: `${MOD_LABEL}⇧D`, description: "Split horizontal" },
      { keys: `${MOD_LABEL}⇧W`, description: "Close split" },
      { keys: `${MOD_LABEL}⇧←→↑↓`, description: "Focus split in direction" },
      { keys: `${MOD_LABEL}⌥←→↑↓`, description: "Move pane to split in direction" },
    ]},
    { section: "Terminal", items: [
      { keys: IS_MAC ? `${MOD_LABEL}C` : `Ctrl+Shift+C`, description: "Copy selection" },
      { keys: IS_MAC ? `${MOD_LABEL}V` : `Ctrl+Shift+V`, description: "Paste" },
      { keys: IS_MAC ? `${MOD_LABEL}⌫` : `Ctrl+Backspace`, description: "Kill line" },
      { keys: IS_MAC ? `${MOD_LABEL}←` : `Home`, description: "Beginning of line" },
      { keys: IS_MAC ? `${MOD_LABEL}→` : `End`, description: "End of line" },
      { keys: `Shift+Enter`, description: "Newline (no submit)" },
      { keys: `Escape`, description: "Send Escape to terminal" },
    ]},
    { section: "Forms", items: [
      { keys: MOD_ENTER_HINT, description: "Submit form" },
    ]},
  ];

  const prPanelShortcuts = { section: "PR Panel", items: [
    { keys: `${MOD_LABEL}⇧P`, description: "Open PR panel / Create PR" },
    { keys: `o`, description: "Open PR in browser" },
    { keys: `m`, description: "Merge" },
    { keys: `s`, description: "Cycle merge strategy" },
    { keys: `r`, description: "Refresh CI checks" },
    { keys: `R`, description: "Mark as ready (draft)" },
    { keys: `f`, description: "Send failures to agent" },
    { keys: `1–9`, description: "Open CI check in browser" },
    { keys: `Esc`, description: "Close panel" },
  ]};

  const sidebarShortcuts = { section: "Sidebar Navigation", items: [
    { keys: `j / ↓`, description: "Next item" },
    { keys: `k / ↑`, description: "Previous item" },
    { keys: `Enter`, description: "Select / open" },
    { keys: `a`, description: "Archive" },
    { keys: `dd`, description: "Delete" },
    { keys: `r`, description: "Review session changes" },
    { keys: `E`, description: "Rename" },
    { keys: `e`, description: "Edit task" },
    { keys: `R`, description: "Restart" },
    { keys: `o`, description: "Open PR" },
    { keys: `st`, description: "Status → Todo" },
    { keys: `sp`, description: "Status → In Progress" },
    { keys: `sr`, description: "Status → In Review" },
    { keys: `sd`, description: "Status → Done" },
    { keys: `ss`, description: "Start session" },
  ]};

  const reviewShortcuts = { section: "Review (Diff)", items: [
    { keys: `j/k`, description: "Navigate files / move cursor" },
    { keys: `Enter`, description: "Focus diff body" },
    { keys: `Esc`, description: "Back to file list" },
    { keys: `]/[`, description: "Next/prev hunk" },
    { keys: `Ctrl+n/p`, description: "Next/prev file" },
    { keys: `m`, description: "Mark file viewed" },
    { keys: `Shift+↓/↑`, description: "Select lines" },
    { keys: `v`, description: "Visual select" },
    { keys: `c`, description: "Comment on line/selection" },
    { keys: `d/f b`, description: "Page down / up" },
    { keys: `g/G`, description: "Top / bottom" },
    { keys: `u`, description: "Split / unified" },
    { keys: `e`, description: "Edit file" },
    { keys: `x`, description: "Expand collapsed context" },
    { keys: `r`, description: "Refresh" },
    { keys: MOD_ENTER_HINT, description: "Send feedback" },
  ]};

  const visibleShortcuts = $derived(
    getActiveZone() === "sidebar"
      ? [sidebarShortcuts, ...shortcuts]
      : [reviewShortcuts, prPanelShortcuts, ...shortcuts]
  );

  const filteredShortcuts = $derived(
    filterShortcuts(visibleShortcuts, searchQuery)
  );

  // Reset search query when dialog closes
  $effect(() => {
    if (!open) {
      searchQuery = "";
    }
  });

  // Auto-focus the search input when dialog opens
  $effect(() => {
    if (open && searchInput) {
      // Use a microtask to ensure the DOM is ready
      queueMicrotask(() => searchInput?.focus());
    }
  });
</script>

<Dialog {open} {onOpenChange} title="Keyboard Shortcuts" description="List of keyboard shortcuts available in planeai." class="w-full max-w-2xl rounded-xl p-5 outline-none">
  <h2 class="flex-shrink-0 text-sm font-medium text-t1 mb-4">Keyboard Shortcuts</h2>
  <input
    bind:this={searchInput}
    bind:value={searchQuery}
    type="text"
    placeholder="Search shortcuts..."
    aria-label="Search shortcuts"
    class="flex-shrink-0 mb-4 w-full rounded-md border border-border bg-panel-hi px-3 py-1.5 text-sm text-t1 placeholder:text-t3 outline-none focus:ring-1 focus:ring-accent"
  />
  {#if filteredShortcuts.length === 0}
    <p class="text-sm text-t3 text-center py-6">No shortcuts found</p>
  {:else}
    <div class="flex-1 min-h-0 overflow-y-auto overscroll-contain">
      <div class="grid grid-cols-2 gap-x-6 gap-y-4">
        {#each filteredShortcuts as group}
          <div>
            {#if group.section}
              <h3 class="text-xs font-medium text-t3 uppercase tracking-wide mb-1.5">{group.section}</h3>
            {/if}
            <div class="space-y-1">
              {#each group.items as shortcut}
                <div class="flex items-center justify-between py-1">
                  <span class="text-sm text-t2">{shortcut.description}</span>
                  <kbd class="rounded border border-border bg-panel-hi px-1.5 py-0.5 text-xs text-t2 font-mono">{shortcut.keys}</kbd>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</Dialog>
