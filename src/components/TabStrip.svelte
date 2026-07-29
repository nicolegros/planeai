<script lang="ts">
  /**
   * TabStrip — shared inline tab bar used by both Titlebar and split panes.
   * Renders tabs with border-b-2 underline style, a "+" button, and an optional close button.
   */
  import { Bot, Terminal, GitCompare, FileCode, X } from "@lucide/svelte";
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    tabs: Tab[];
    activeTabIndex: number;
    focused?: boolean;
    showAddButton?: boolean;
    showCloseButton?: boolean;
    draggable?: boolean;
    onSelectTab: (index: number) => void;
    onAddTab?: () => void;
    onClose?: () => void;
    onTabDragStart?: (e: DragEvent, tabIndex: number) => void;
    onTabDrop?: (e: DragEvent, insertIndex: number) => void;
    onTabDragOver?: (e: DragEvent) => void;
  }

  let { tabs, activeTabIndex, focused = true, showAddButton = true, showCloseButton = false, draggable = false, onSelectTab, onAddTab, onClose, onTabDragStart, onTabDrop, onTabDragOver }: Props = $props();

  const TAB_ICONS: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompare, file: FileCode, terminal: Terminal };

  let dropTargetIndex = $state<number | null>(null);

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropTargetIndex = index;
    onTabDragOver?.(e);
  }

  function handleDragLeave() {
    dropTargetIndex = null;
  }

  function handleDrop(e: DragEvent, index: number) {
    e.preventDefault();
    dropTargetIndex = null;
    onTabDrop?.(e, index);
  }
</script>

<div class="flex items-stretch h-[38px] flex-1" role="tablist">
  {#each tabs as tab, i (tab.index)}
    {@const Icon = TAB_ICONS[tab.icon ?? 'terminal'] ?? Terminal}
    {@const isActive = tab.index === activeTabIndex}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="relative flex items-stretch"
      ondragover={draggable ? (e) => handleDragOver(e, i) : undefined}
      ondragleave={draggable ? handleDragLeave : undefined}
      ondrop={draggable ? (e) => handleDrop(e, i) : undefined}
    >
      {#if dropTargetIndex === i}
        <div class="absolute left-0 top-[8px] bottom-[8px] w-[2px] bg-accent rounded-full"></div>
      {/if}
      <button
        role="tab"
        aria-selected={isActive}
        class="flex items-center gap-[7px] px-[13px] text-[12.5px] font-medium select-none border-b-2 transition-colors
          {isActive && focused ? 'border-accent text-t1' : isActive ? 'border-transparent text-t1' : 'border-transparent text-t2 hover:text-t1'}"
        draggable={draggable ? "true" : undefined}
        ondragstart={draggable ? (e) => onTabDragStart?.(e, tab.index) : undefined}
        onclick={() => onSelectTab(tab.index)}
      >
        <Icon size={13} class={isActive && focused ? 'text-accent' : 'text-t3'} />
        {tab.label}
      </button>
    </div>
  {/each}
  <!-- Drop zone after last tab -->
  {#if draggable}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="relative w-2 shrink-0"
      ondragover={(e) => handleDragOver(e, tabs.length)}
      ondragleave={handleDragLeave}
      ondrop={(e) => handleDrop(e, tabs.length)}
    >
      {#if dropTargetIndex === tabs.length}
        <div class="absolute left-0 top-[8px] bottom-[8px] w-[2px] bg-accent rounded-full"></div>
      {/if}
    </div>
  {/if}
  {#if showAddButton && onAddTab}
    <button class="flex items-center px-[11px] text-[15px] text-t3 hover:text-t2" aria-label="New tab" onclick={onAddTab}>+</button>
  {/if}
  <!-- Spacer -->
  <div class="flex-1"></div>
  {#if showCloseButton && onClose}
    <button
      class="flex items-center justify-center w-[28px] h-[28px] my-auto mr-1 rounded-[6px] text-t3 hover:text-t1 hover:bg-panel-hi transition-colors"
      title="Close split (migrate tabs to sibling)"
      onclick={onClose}
    >
      <X size={13} />
    </button>
  {/if}
</div>
