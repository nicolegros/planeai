<script lang="ts">
  import { IS_MAC, MOD_LABEL } from "../lib/keyboard";
  import type { PaneTab } from "../lib/task-workspace-layout.svelte";
  import type { PluginInventory, PluginUiContribution } from "../lib/types";
  import type { PluginSessionContext } from "../lib/plugin-sdk";
  import PluginContributionHost from "./PluginContributionHost.svelte";
  import TabStrip from "./TabStrip.svelte";

  interface TitlebarContribution { plugin: PluginInventory; contribution: PluginUiContribution; }
  interface Props {
    projectName: string | null; sessionName: string | null; sidebarVisible: boolean;
    tabs: PaneTab[]; activeTabId: string;
    symphonyStatus: { active: boolean; slots_used: number; max_concurrent: number } | null;
    runningCount: number; activeProvider: string | null;
    onSelectTab: (tabId: string) => void;
    onAddTab: () => void; onTabDoubleClick?: (tabId: string) => void; onOpenCommand?: () => void;
    onTabDragStart?: (e: DragEvent, tabId: string) => void; onTabDrop?: (e: DragEvent, insertIndex: number) => void; onTabDragEnd?: () => void;
    titlebarContributions?: TitlebarContribution[]; titlebarSession?: PluginSessionContext;
    onOpenTitlebarContribution?: (pluginId: string, contributionId: string) => void;
  }
  let { projectName, sessionName, tabs, activeTabId, symphonyStatus: _symphonyStatus, runningCount, activeProvider, onSelectTab, onAddTab, onTabDoubleClick, onOpenCommand, onTabDragStart, onTabDrop, onTabDragEnd, titlebarContributions = [], titlebarSession, onOpenTitlebarContribution }: Props = $props();
  const platformPadding = IS_MAC ? "pl-[72px]" : "pr-36";
</script>
<header data-tauri-drag-region class="h-[38px] flex items-center gap-3 px-[13px] shrink-0 bg-chrome border-b border-border {platformPadding}">
  {#if projectName || sessionName}
    <div data-tauri-drag-region class="flex items-center gap-2 text-[12.5px] select-none pointer-events-none shrink-0">
      {#if projectName}<span class="text-t2">{projectName}</span>{/if}
      {#if projectName && sessionName}<span class="text-t3">/</span>{/if}
      {#if sessionName}<span class="text-t1 font-medium">{sessionName}</span>{/if}
    </div>
    {#if activeProvider}<span class="font-mono text-[10px] tracking-[.04em] text-t3 border border-border rounded-[5px] px-1.5 py-0.5 select-none">{activeProvider.toUpperCase()}</span>{/if}
    <span class="w-px h-[18px] bg-border shrink-0"></span>
  {/if}
  <TabStrip {tabs} {activeTabId} onSelectTab={onSelectTab} onAddTab={onAddTab} {onTabDoubleClick} draggable={!!onTabDragStart} {onTabDragStart} {onTabDrop} {onTabDragEnd} />
  <div class="ml-auto flex items-center gap-3 shrink-0">
    {#if runningCount > 0}<span class="flex items-center gap-1.5 text-[11.5px] text-t2 select-none"><span class="size-[7px] rounded-full bg-status-running" style="animation:pulse-dot 1.6s ease-in-out infinite"></span>{runningCount} running</span>{/if}
    {#each titlebarContributions as item (`${item.plugin.id}:${item.contribution.id}`)}
      <div class="h-[25px] shrink-0 overflow-hidden" style="width: 88px; min-width: 0; max-width: 88px" data-plugin-titlebar-entry={`${item.plugin.id}:${item.contribution.id}`}>
        <PluginContributionHost plugin={item.plugin} contribution={item.contribution} session={titlebarSession} onNavigate={onOpenTitlebarContribution ?? (() => {})} onClose={() => {}} />
      </div>
    {/each}
    <button class="font-mono text-[10.5px] text-t2 border border-border rounded-[6px] px-[7px] py-[3px] bg-panel-hi select-none hover:text-t1 hover:border-border-s transition-colors" title="Open command palette" onclick={(e) => { e.stopPropagation(); onOpenCommand?.(); }}>{MOD_LABEL}K</button>
  </div>
</header>
