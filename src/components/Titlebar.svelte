<script lang="ts">
  import { IS_MAC } from "../lib/keyboard";

  interface Props {
    projectName: string | null;
    sessionName: string | null;
    sidebarVisible: boolean;
  }

  let { projectName, sessionName, sidebarVisible }: Props = $props();

  const platformPadding = IS_MAC ? "pl-20" : "pr-36";
</script>

<header
  data-tauri-drag-region
  class="h-10 flex items-center {platformPadding} shrink-0 bg-surface-100 dark:bg-surface-900 border-b border-surface-200 dark:border-surface-800"
>
  {#if projectName || sessionName}
    <span class="text-xs text-surface-700 dark:text-surface-200 select-none pointer-events-none">
      {#if projectName}<span>{projectName}</span>{/if}
      {#if projectName && sessionName}<span class="mx-1.5">/</span>{/if}
      {#if sessionName}<span class="text-surface-700 dark:text-surface-200">{sessionName}</span>{/if}
    </span>
  {/if}
</header>
