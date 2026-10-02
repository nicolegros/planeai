<script lang="ts">
  /**
   * TabStrip — shared inline tab bar used by both Titlebar and split panes.
   * Renders tabs with border-b-2 underline style, a "+" button, and an optional close button.
   */
  import { Bot, Terminal, GitCompare, FileCode, X } from "@lucide/svelte";
  import type { PaneTab } from "../lib/task-workspace-layout.svelte";
  import { tabDrag } from "../lib/tab-drag.svelte";

  interface Props {
    tabs: PaneTab[];
    activeTabId: string;
    focused?: boolean;
    showAddButton?: boolean;
    showCloseButton?: boolean;
    /** The pane these tabs belong to; set it with `onTabPress` to make tabs draggable. */
    paneId?: string;
    onSelectTab: (tabId: string) => void;
    onAddTab?: () => void;
    onClose?: () => void;
    onTabPress?: (e: PointerEvent, tabId: string) => void;
    onTabDoubleClick?: (tabId: string) => void;
  }

  let { tabs, activeTabId, focused = true, showAddButton = true, showCloseButton = false, paneId, onSelectTab, onAddTab, onClose, onTabPress, onTabDoubleClick }: Props = $props();

  const TAB_ICONS: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompare, file: FileCode, terminal: Terminal };

  const draggable = $derived(!!paneId && !!onTabPress);
  /** Insert position a dragged tab targets in this strip: before tab `i`, or `tabs.length` for the end. */
  const dropIndex = $derived.by(() => {
    const target = tabDrag.target;
    return draggable && target?.kind === "strip" && target.paneId === paneId ? target.index : null;
  });
</script>

<!-- Anywhere on the strip past the tabs drops a dragged tab at the end. -->
<div
  class="flex items-stretch h-[38px] flex-1"
  role="tablist"
  aria-label="Pane tabs"
  data-tab-strip={draggable ? paneId : undefined}
  data-tab-count={draggable ? tabs.length : undefined}
>
  {#each tabs as tab, i (tab.id)}
    {@const Icon = TAB_ICONS[tab.icon ?? 'terminal'] ?? Terminal}
    {@const isActive = tab.id === activeTabId}
    <div class="relative flex items-stretch" data-tab-index={i}>
      {#if dropIndex === i}
        <div class="absolute left-0 top-[8px] bottom-[8px] w-[2px] bg-accent rounded-full"></div>
      {:else if dropIndex === tabs.length && i === tabs.length - 1}
        <div class="absolute right-0 top-[8px] bottom-[8px] w-[2px] bg-accent rounded-full"></div>
      {/if}
      <button
        role="tab"
        aria-selected={isActive}
        class="flex items-center gap-[7px] px-[13px] text-[12.5px] font-medium select-none border-b-2 transition-colors
          {isActive && focused ? 'border-accent text-t1' : isActive ? 'border-transparent text-t1' : 'border-transparent text-t2 hover:text-t1'}"
        onpointerdown={draggable ? (e) => onTabPress?.(e, tab.id) : undefined}
        onclick={() => onSelectTab(tab.id)}
        ondblclick={onTabDoubleClick ? () => onTabDoubleClick(tab.id) : undefined}
      >
        <Icon size={13} class={isActive && focused ? 'text-accent' : 'text-t3'} />
        {tab.label}
        {#if tab.detail}<span class="-ml-[3px] font-normal text-t3">· {tab.detail}</span>{/if}
      </button>
    </div>
  {/each}
  {#if showAddButton && onAddTab}
    <button class="flex items-center px-[11px] text-[15px] text-t3 hover:text-t2" aria-label="New tab" onclick={onAddTab}>+</button>
  {/if}
  <!-- Spacer -->
  <div class="flex-1"></div>
  {#if showCloseButton && onClose}
    <button
      class="flex items-center justify-center w-[28px] h-[28px] my-auto mr-1 rounded-[6px] text-t3 hover:text-t1 hover:bg-panel-hi transition-colors"
      title="Close split (migrate tabs to sibling)"
      aria-label="Close split pane"
      onclick={onClose}
    >
      <X size={13} />
    </button>
  {/if}
</div>
