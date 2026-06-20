<script lang="ts">
  import { Dialog } from "bits-ui";
  import { IS_MAC, MOD_LABEL, MOD_ENTER_HINT } from "../lib/keyboard";
  import { getActiveZone } from "../lib/focus.svelte";

  interface Props {
    open: boolean;
    onOpenChange: (open: boolean) => void;
  }

  let { open, onOpenChange }: Props = $props();

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
    ]},
    { section: "Tabs", items: [
      { keys: `${MOD_LABEL}T`, description: "New tab" },
      { keys: `${MOD_LABEL}W`, description: "Close tab" },
      { keys: `${MOD_LABEL}⇧]`, description: "Next tab" },
      { keys: `${MOD_LABEL}⇧[`, description: "Previous tab" },
    ]},
    { section: "View", items: [
      { keys: `${MOD_LABEL}B`, description: "Toggle sidebar" },
      { keys: `${MOD_LABEL}⇧S`, description: "Focus sessions panel" },
      { keys: `${MOD_LABEL}⇧T`, description: "Focus tasks panel" },
      { keys: `${MOD_LABEL}R`, description: "Refresh tasks" },
      { keys: `${MOD_LABEL}⇧R`, description: "Open Review" },
      { keys: `${MOD_LABEL}E`, description: "Toggle file explorer" },
      { keys: `${MOD_LABEL}D`, description: "Toggle diff" },
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

  const sidebarShortcuts = { section: "Sidebar Navigation", items: [
    { keys: `j / ↓`, description: "Next item" },
    { keys: `k / ↑`, description: "Previous item" },
    { keys: `Enter`, description: "Select / open" },
    { keys: `a`, description: "Archive" },
    { keys: `dd`, description: "Delete" },
    { keys: `r`, description: "Review session changes" },
    { keys: `n`, description: "Rename" },
    { keys: `e`, description: "Edit task" },
    { keys: `R`, description: "Restart" },
    { keys: `o`, description: "Open PR" },
    { keys: `st`, description: "Status → Todo" },
    { keys: `sp`, description: "Status → In Progress" },
    { keys: `sr`, description: "Status → In Review" },
    { keys: `sd`, description: "Status → Done" },
    { keys: `ss`, description: "Start session" },
  ]};

  const visibleShortcuts = $derived(
    getActiveZone() === "sidebar"
      ? [sidebarShortcuts, ...shortcuts]
      : shortcuts
  );
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-50" />
    <Dialog.Content
      class="fixed left-1/2 top-1/2 z-50 w-full max-w-sm -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-200 bg-surface-50 p-5 shadow-lg dark:border-surface-700 dark:bg-surface-900 outline-none"
    >
      <Dialog.Title class="text-sm font-medium text-surface-900 dark:text-surface-50 mb-4">Keyboard Shortcuts</Dialog.Title>
      <Dialog.Description class="sr-only">List of keyboard shortcuts available in planeai.</Dialog.Description>
      <div class="space-y-4">
        {#each visibleShortcuts as group}
          <div>
            <h3 class="text-xs font-medium text-surface-500 dark:text-surface-400 uppercase tracking-wide mb-1.5">{group.section}</h3>
            <div class="space-y-1">
              {#each group.items as shortcut}
                <div class="flex items-center justify-between py-1">
                  <span class="text-sm text-surface-700 dark:text-surface-300">{shortcut.description}</span>
                  <kbd class="rounded border border-surface-300 dark:border-surface-600 bg-surface-100 dark:bg-surface-800 px-1.5 py-0.5 text-xs text-surface-600 dark:text-surface-400 font-mono">{shortcut.keys}</kbd>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
