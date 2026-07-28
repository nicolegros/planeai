<script lang="ts">
  /**
   * SplitLeafTabs — per-leaf tab bar shown when there are multiple splits.
   * Renders tab buttons, a close-split "X" button, and handles tab drag source.
   */
  import { Bot, Terminal, GitCompare, FileCode, X } from "@lucide/svelte";
  import type { LeafNode } from "../lib/split-tree.svelte";
  import { getFocusedLeafId, setFocusedLeaf, setLeafActiveTab, closeSplit, getAllLeaves } from "../lib/split-tree.svelte";

  interface TabInfo {
    sessionId: string;
    label: string;
    icon: string;
  }

  interface Props {
    leaf: LeafNode;
    tabs: TabInfo[];
    onTabDragStart: (e: DragEvent, sessionId: string, leafId: string) => void;
    onTabDrop: (e: DragEvent, leafId: string, insertIndex: number) => void;
    onTabDragOver: (e: DragEvent) => void;
  }

  let { leaf, tabs, onTabDragStart, onTabDrop, onTabDragOver }: Props = $props();

  const isFocused = $derived(getFocusedLeafId() === leaf.id);
  const showCloseButton = $derived(getAllLeaves().length > 1);

  const TAB_ICONS: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompare, file: FileCode, terminal: Terminal };

  function handleTabClick(index: number) {
    setFocusedLeaf(leaf.id);
    setLeafActiveTab(leaf.id, index);
  }

  function handleClose() {
    closeSplit(leaf.id);
  }

  let dropTargetIndex = $state<number | null>(null);

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropTargetIndex = index;
    onTabDragOver(e);
  }

  function handleDragLeave() {
    dropTargetIndex = null;
  }

  function handleDrop(e: DragEvent, index: number) {
    e.preventDefault();
    dropTargetIndex = null;
    onTabDrop(e, leaf.id, index);
  }
</script>

<div
  class="split-leaf-tabs"
  class:focused={isFocused}
  role="tablist"
  aria-label="Split pane tabs"
>
  {#each tabs as tab, i (tab.sessionId)}
    {@const Icon = TAB_ICONS[tab.icon] ?? Terminal}
    {@const isActive = i === leaf.activeTab}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="split-tab-wrapper"
      ondragover={(e) => handleDragOver(e, i)}
      ondragleave={handleDragLeave}
      ondrop={(e) => handleDrop(e, i)}
    >
      {#if dropTargetIndex === i}
        <div class="drop-indicator"></div>
      {/if}
      <button
        role="tab"
        aria-selected={isActive}
        class="split-tab {isActive ? 'active' : ''}"
        draggable="true"
        ondragstart={(e) => onTabDragStart(e, tab.sessionId, leaf.id)}
        onclick={() => handleTabClick(i)}
      >
        <Icon size={12} class="shrink-0 {isActive ? 'text-accent' : 'text-t3'}" />
        <span class="truncate">{tab.label}</span>
      </button>
    </div>
  {/each}
  <!-- Drop zone after last tab -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="split-tab-drop-end"
    ondragover={(e) => handleDragOver(e, tabs.length)}
    ondragleave={handleDragLeave}
    ondrop={(e) => handleDrop(e, tabs.length)}
  >
    {#if dropTargetIndex === tabs.length}
      <div class="drop-indicator"></div>
    {/if}
  </div>
  <!-- Spacer to push close button right -->
  <div class="flex-1"></div>
  {#if showCloseButton}
    <button
      class="split-close-btn"
      title="Close split (migrate tabs to sibling)"
      onclick={handleClose}
    >
      <X size={12} />
    </button>
  {/if}
</div>

<style>
  .split-leaf-tabs {
    display: flex;
    align-items: stretch;
    height: 30px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-chrome);
    padding: 0 4px;
    gap: 1px;
    overflow-x: auto;
    overflow-y: hidden;
  }

  .split-leaf-tabs.focused {
    border-bottom-color: var(--color-accent);
  }

  .split-tab-wrapper {
    position: relative;
    display: flex;
    align-items: stretch;
  }

  .split-tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--color-t2);
    cursor: pointer;
    white-space: nowrap;
    user-select: none;
    border-radius: 4px 4px 0 0;
    transition: color 0.1s, background 0.1s;
  }

  .split-tab:hover {
    color: var(--color-t1);
    background: var(--color-panel-hi);
  }

  .split-tab.active {
    color: var(--color-t1);
    background: var(--color-main);
  }

  .split-close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin: auto 2px;
    border-radius: 4px;
    color: var(--color-t3);
    cursor: pointer;
    transition: color 0.1s, background 0.1s;
  }

  .split-close-btn:hover {
    color: var(--color-t1);
    background: var(--color-panel-hi);
  }

  .split-tab-drop-end {
    position: relative;
    width: 8px;
    flex-shrink: 0;
  }

  .drop-indicator {
    position: absolute;
    left: 0;
    top: 4px;
    bottom: 4px;
    width: 2px;
    background: var(--color-accent);
    border-radius: 1px;
  }
</style>
