<script lang="ts">
  import { IS_MAC, MOD_LABEL } from "../lib/keyboard";
  import { Bot, Terminal, GitCompare, FileCode } from "@lucide/svelte";
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    projectName: string | null;
    sessionName: string | null;
    sidebarVisible: boolean;
    tabs: Tab[];
    activeTabIndex: number;
    prUrl: string | null;
    prState: string | null;
    ciStatus: "passing" | "failing" | "pending" | null;
    hasChanges: boolean;
    sessionId: string | null;
    symphonyStatus: { active: boolean; slots_used: number; max_concurrent: number } | null;
    runningCount: number;
    activeProvider: string | null;
    onSelectTab: (index: number) => void;
    onCloseTab: (index: number) => void;
    onAddTab: () => void;
    onCreatePr?: () => void;
    onOpenCommand?: () => void;
    onTogglePrPanel?: () => void;
  }

  let { projectName, sessionName, sidebarVisible, tabs, activeTabIndex, prUrl, prState, ciStatus, hasChanges, sessionId, symphonyStatus, runningCount, activeProvider, onSelectTab, onCloseTab, onAddTab, onCreatePr, onOpenCommand, onTogglePrPanel }: Props = $props();

  const platformPadding = IS_MAC ? "pl-[72px]" : "pr-36";

  let isMerged = $derived(prState === "merged");
  let isDraft = $derived(prState === "draft");

  const TAB_ICONS: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompare, file: FileCode, terminal: Terminal };
</script>

<header
  data-tauri-drag-region
  class="h-[38px] flex items-center gap-3 px-[13px] shrink-0 bg-chrome border-b border-border {platformPadding}"
>
  <!-- Breadcrumb: project / session + provider tag -->
  {#if projectName || sessionName}
    <div data-tauri-drag-region class="flex items-center gap-2 text-[12.5px] select-none pointer-events-none shrink-0">
      {#if projectName}<span class="text-t2">{projectName}</span>{/if}
      {#if projectName && sessionName}<span class="text-t3">/</span>{/if}
      {#if sessionName}<span class="text-t1 font-medium">{sessionName}</span>{/if}
    </div>
    {#if activeProvider}
      <span class="font-mono text-[10px] tracking-[.04em] text-t3 border border-border rounded-[5px] px-1.5 py-0.5 select-none">{activeProvider.toUpperCase()}</span>
    {/if}
    <span class="w-px h-[18px] bg-border shrink-0"></span>
  {/if}

  <!-- Inline tabs -->
  <div data-tauri-drag-region class="flex items-stretch h-[38px] flex-1" role="tablist">
    {#each tabs as tab (tab.index)}
      {@const Icon = TAB_ICONS[tab.icon ?? 'terminal'] ?? Terminal}
      <button
        role="tab"
        aria-selected={tab.index === activeTabIndex}
        class="flex items-center gap-[7px] px-[13px] text-[12.5px] font-medium select-none border-b-2 transition-colors
          {tab.index === activeTabIndex ? 'border-accent text-t1' : 'border-transparent text-t2 hover:text-t1'}"
        onclick={() => onSelectTab(tab.index)}
      >
        <Icon size={13} class={tab.index === activeTabIndex ? 'text-accent' : 'text-t3'} />
        {tab.label}
      </button>
    {/each}
    <button class="flex items-center px-[11px] text-[15px] text-t3 hover:text-t2" onclick={onAddTab}>+</button>
  </div>

  <!-- Right cluster -->
  <div class="ml-auto flex items-center gap-3 shrink-0">
    {#if runningCount > 0}
      <span class="flex items-center gap-1.5 text-[11.5px] text-t2 select-none">
        <span class="size-[7px] rounded-full bg-status-running" style="animation:pulse-dot 1.6s ease-in-out infinite"></span>
        {runningCount} running
      </span>
    {/if}

    <!-- PR controls -->
    {#if prUrl}
      <button
        class="flex items-center gap-[7px] h-[25px] px-[9px] rounded-[7px] text-[11.5px] font-medium
          {isMerged ? 'bg-[rgba(188,140,255,0.18)] text-[#bc8cff]' : isDraft ? 'bg-panel-hi text-t2' : prState === 'closed' ? 'bg-status-exited/15 text-status-exited' : 'bg-[rgba(63,185,80,0.18)] text-status-running'}"
        onclick={onTogglePrPanel}
      >
        <span class="size-[7px] rounded-full {isMerged ? 'bg-[#bc8cff]' : isDraft ? 'bg-t3' : prState === 'closed' ? 'bg-status-exited' : 'bg-status-running'}"></span>
        PR{prUrl?.match(/#(\d+)/)?.[0] ?? ''}
        {#if ciStatus}
          <span class="size-[5px] rounded-full {ciStatus === 'failing' ? 'bg-status-exited' : ciStatus === 'passing' ? 'bg-status-running' : 'bg-status-review animate-pulse'}"></span>
        {/if}
      </button>
    {:else if hasChanges && onCreatePr}
      <button
        class="flex items-center gap-1.5 h-[25px] px-[10px] rounded-[7px] text-[11.5px] font-medium text-t2 border border-border hover:bg-panel-hi transition-colors"
        onclick={onCreatePr}
      >＋ Create PR</button>
    {/if}

    <button
      class="font-mono text-[10.5px] text-t2 border border-border rounded-[6px] px-[7px] py-[3px] bg-panel-hi select-none hover:text-t1 hover:border-border-s transition-colors"
      title="Open command palette"
      onclick={(e) => { e.stopPropagation(); onOpenCommand?.(); }}
    >{MOD_LABEL}K</button>
  </div>
</header>
