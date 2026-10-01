<script lang="ts">
  /**
   * TabStrip — shared inline tab bar used by both Titlebar and split panes.
   * Renders tabs with border-b-2 underline style, a "+" button, and an optional close button.
   */
  import { Bot, Terminal, GitCompare, FileCode, X } from "@lucide/svelte";
  import type { PaneTab } from "../lib/task-workspace-layout.svelte";

  interface Props {
    tabs: PaneTab[];
    activeTabId: string;
    focused?: boolean;
    showAddButton?: boolean;
    showCloseButton?: boolean;
    draggable?: boolean;
    onSelectTab: (tabId: string) => void;
    onAddTab?: () => void;
    onClose?: () => void;
    onTabDragStart?: (e: DragEvent, tabId: string) => void;
    onTabDrop?: (e: DragEvent, insertIndex: number) => void;
    onTabDragOver?: (e: DragEvent) => void;
    onTabDragEnd?: () => void;
    onTabDoubleClick?: (tabId: string) => void;
  }

  let { tabs, activeTabId, focused = true, showAddButton = true, showCloseButton = false, draggable = false, onSelectTab, onAddTab, onClose, onTabDragStart, onTabDrop, onTabDragOver, onTabDragEnd, onTabDoubleClick }: Props = $props();

  const TAB_ICONS: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompare, file: FileCode, terminal: Terminal };

  /** Insert position a drag currently targets: before tab `i`, or `tabs.length` for the end. */
  let dropTargetIndex = $state<number | null>(null);

  /** Before or after a tab, by which half of it the pointer is over. */
  function indexAtTab(e: DragEvent, i: number): number {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return e.clientX > rect.left + rect.width / 2 ? i + 1 : i;
  }

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropTargetIndex = index;
    onTabDragOver?.(e);
  }

  function handleDragLeave(e: DragEvent) {
    // Moving between the strip's own children is not leaving it.
    if (e.relatedTarget instanceof Node && (e.currentTarget as HTMLElement).contains(e.relatedTarget)) return;
    dropTargetIndex = null;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    const index = dropTargetIndex ?? tabs.length;
    dropTargetIndex = null;
    onTabDrop?.(e, index);
  }
</script>

<!-- The whole strip accepts drops; anywhere past the tabs means the end. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex items-stretch h-[38px] flex-1"
  role="tablist"
  aria-label="Pane tabs"
  tabindex="-1"
  ondragover={draggable ? (e) => handleDragOver(e, tabs.length) : undefined}
  ondragleave={draggable ? handleDragLeave : undefined}
  ondrop={draggable ? handleDrop : undefined}
>
  {#each tabs as tab, i (tab.id)}
    {@const Icon = TAB_ICONS[tab.icon ?? 'terminal'] ?? Terminal}
    {@const isActive = tab.id === activeTabId}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="relative flex items-stretch"
      ondragover={draggable ? (e) => { e.stopPropagation(); handleDragOver(e, indexAtTab(e, i)); } : undefined}
    >
      {#if dropTargetIndex === i}
        <div class="absolute left-0 top-[8px] bottom-[8px] w-[2px] bg-accent rounded-full"></div>
      {:else if dropTargetIndex === tabs.length && i === tabs.length - 1}
        <div class="absolute right-0 top-[8px] bottom-[8px] w-[2px] bg-accent rounded-full"></div>
      {/if}
      <button
        role="tab"
        aria-selected={isActive}
        class="flex items-center gap-[7px] px-[13px] text-[12.5px] font-medium select-none border-b-2 transition-colors
          {isActive && focused ? 'border-accent text-t1' : isActive ? 'border-transparent text-t1' : 'border-transparent text-t2 hover:text-t1'}"
        draggable={draggable ? "true" : undefined}
        ondragstart={draggable ? (e) => onTabDragStart?.(e, tab.id) : undefined}
        ondragend={draggable ? () => { dropTargetIndex = null; onTabDragEnd?.(); } : undefined}
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
