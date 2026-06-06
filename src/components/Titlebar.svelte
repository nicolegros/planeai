<script lang="ts">
  import { IS_MAC } from "../lib/keyboard";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { GitPullRequest } from "@lucide/svelte";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    projectName: string | null;
    sessionName: string | null;
    sidebarVisible: boolean;
    tabs: Tab[];
    activeTabIndex: number;
    prUrl: string | null;
    onSelectTab: (index: number) => void;
    onCloseTab: (index: number) => void;
    onAddTab: () => void;
  }

  let { projectName, sessionName, sidebarVisible, tabs, activeTabIndex, prUrl, onSelectTab, onCloseTab, onAddTab }: Props = $props();

  const platformPadding = IS_MAC ? "pl-20" : "pr-36";
</script>

<header
  data-tauri-drag-region
  class="h-10 flex items-center {platformPadding} shrink-0 bg-surface-100 dark:bg-surface-900 border-b border-surface-200 dark:border-surface-800"
>
  {#if projectName || sessionName}
    <span class="text-xs text-surface-700 dark:text-surface-200 select-none pointer-events-none whitespace-nowrap mr-3">
      {#if projectName}<span>{projectName}</span>{/if}
      {#if projectName && sessionName}<span class="mx-1.5">/</span>{/if}
      {#if sessionName}<span>{sessionName}</span>{/if}
    </span>
    <span class="w-px h-4 bg-surface-300 dark:bg-surface-700 shrink-0 mr-3"></span>
  {/if}

  <div class="flex-1 min-w-0 relative">
    <div class="overflow-x-auto scrollbar-hide">
      <TabBar {tabs} {activeTabIndex} onSelect={onSelectTab} onClose={onCloseTab} onAdd={onAddTab} />
    </div>
    <div class="pointer-events-none absolute inset-y-0 right-0 w-6 bg-gradient-to-l from-surface-100 dark:from-surface-900 to-transparent"></div>
  </div>

  {#if prUrl}
    <button
      class="ml-2 mr-3 shrink-0 flex items-center gap-1 px-2 py-1 rounded text-xs text-primary-600 dark:text-primary-400 hover:bg-surface-200 dark:hover:bg-surface-800 transition-colors"
      title="Open pull request"
      tabindex="-1"
      onmousedown={(e: MouseEvent) => e.preventDefault()}
      onclick={() => openUrl(prUrl!)}
    >
      <GitPullRequest class="size-3.5" />
      <span>Open PR</span>
    </button>
  {/if}
</header>
