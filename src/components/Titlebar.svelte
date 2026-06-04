<script lang="ts">
  import { IS_MAC } from "../lib/keyboard";
  import { GitCompareArrows } from "@lucide/svelte";

  interface Props {
    projectName: string | null;
    sessionName: string | null;
    sidebarVisible: boolean;
    showDiffButton?: boolean;
    diffActive?: boolean;
    onToggleDiff?: () => void;
  }

  let { projectName, sessionName, sidebarVisible, showDiffButton = false, diffActive = false, onToggleDiff }: Props = $props();

  const platformPadding = IS_MAC ? "pl-20" : "pr-36";
</script>

<header
  data-tauri-drag-region
  class="h-10 flex items-center justify-between {platformPadding} shrink-0 bg-surface-100 dark:bg-surface-900 border-b border-surface-200 dark:border-surface-800"
>
  {#if projectName || sessionName}
    <span class="text-xs text-surface-700 dark:text-surface-200 select-none pointer-events-none">
      {#if projectName}<span>{projectName}</span>{/if}
      {#if projectName && sessionName}<span class="mx-1.5">/</span>{/if}
      {#if sessionName}<span class="text-surface-700 dark:text-surface-200">{sessionName}</span>{/if}
    </span>
  {:else}
    <span></span>
  {/if}

  {#if showDiffButton}
    <button
      class="mr-3 p-1.5 rounded transition-colors {diffActive ? 'bg-surface-300 dark:bg-surface-600 text-surface-900 dark:text-surface-50' : 'text-surface-500 dark:text-surface-400 hover:bg-surface-200 dark:hover:bg-surface-700'}"
      title="Toggle diff view"
      onclick={onToggleDiff}
    >
      <GitCompareArrows size={14} />
    </button>
  {/if}
</header>
