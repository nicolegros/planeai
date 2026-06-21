<script lang="ts">
  import { IS_MAC } from "../lib/keyboard";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { GitPullRequest, Zap, RefreshCw, ChevronDown } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import { pr } from "../lib/api";
  import type { CiCheck } from "../lib/types";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    projectName: string | null;
    sessionName: string | null;
    sidebarVisible: boolean;
    tabs: Tab[];
    activeTabIndex: number;
    prUrl: string | null;
    hasChanges: boolean;
    sessionId: string | null;
    symphonyStatus: { active: boolean; slots_used: number; max_concurrent: number } | null;
    onSelectTab: (index: number) => void;
    onCloseTab: (index: number) => void;
    onAddTab: () => void;
    onCreatePr?: () => void;
  }

  let { projectName, sessionName, sidebarVisible, tabs, activeTabIndex, prUrl, hasChanges, sessionId, symphonyStatus, onSelectTab, onCloseTab, onAddTab, onCreatePr }: Props = $props();

  const platformPadding = IS_MAC ? "pl-20" : "pr-36";

  // ─── CI Checks ──────────────────────────────────────────────────────────────

  let ciChecks = $state<CiCheck[]>([]);
  let ciLoading = $state(false);
  let ciExpanded = $state(false);
  let ciPollTimer: ReturnType<typeof setInterval> | null = null;

  let allChecksConcluded = $derived(
    ciChecks.length > 0 && ciChecks.every((c) => c.status === "completed" || c.conclusion !== null)
  );
  let ciPassedCount = $derived(ciChecks.filter((c) => c.conclusion === "success" || c.conclusion === "neutral" || c.conclusion === "skipped").length);
  let ciFailedCount = $derived(ciChecks.filter((c) => c.conclusion === "failure" || c.conclusion === "cancelled" || c.conclusion === "timed_out").length);

  function ciSummary(): string {
    if (ciFailedCount > 0) return `${ciFailedCount} failed`;
    if (allChecksConcluded) return "All passed";
    return `${ciPassedCount}/${ciChecks.length}`;
  }

  function ciColor(): string {
    if (ciFailedCount > 0) return "text-red-600 dark:text-red-400";
    if (allChecksConcluded) return "text-green-600 dark:text-green-400";
    return "text-yellow-500";
  }

  async function fetchCiChecks() {
    if (!sessionId || !prUrl) return;
    ciLoading = ciChecks.length === 0;
    try {
      ciChecks = await pr.getCiChecks(sessionId);
    } catch {
      // gh not available or no checks
    }
    ciLoading = false;
  }

  function startCiPolling() {
    stopCiPolling();
    if (!sessionId || !prUrl) return;
    fetchCiChecks();
    ciPollTimer = setInterval(() => {
      if (!sessionId || !prUrl || allChecksConcluded) { stopCiPolling(); return; }
      fetchCiChecks();
    }, 30_000);
  }

  function stopCiPolling() {
    if (ciPollTimer) { clearInterval(ciPollTimer); ciPollTimer = null; }
  }

  $effect(() => {
    if (sessionId && prUrl) { startCiPolling(); }
    else { stopCiPolling(); ciChecks = []; }
    return () => stopCiPolling();
  });

  onDestroy(() => stopCiPolling());
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

  {#if symphonyStatus?.active}
    <span
      class="ml-2 shrink-0 flex items-center gap-1 px-2 py-1 rounded text-xs text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-950/30"
      title="Orchestrator running"
    >
      <Zap class="size-3" />
      <span>{symphonyStatus.slots_used}/{symphonyStatus.max_concurrent}</span>
    </span>
  {/if}

  {#if prUrl}
    <button
      class="ml-2 shrink-0 flex items-center gap-1 px-2 py-1 rounded text-xs text-primary-600 dark:text-primary-400 hover:bg-surface-200 dark:hover:bg-surface-800 transition-colors"
      title="Open pull request"
      tabindex="-1"
      onmousedown={(e: MouseEvent) => e.preventDefault()}
      onclick={() => openUrl(prUrl!)}
    >
      <GitPullRequest class="size-3.5" />
      <span>View PR</span>
    </button>

    <!-- CI Checks summary -->
    {#if ciChecks.length > 0}
      <div class="ml-1 shrink-0 relative flex items-center">
        <button
          class="flex items-center gap-1 px-2 py-1 rounded text-xs {ciColor()} hover:bg-surface-200 dark:hover:bg-surface-800 transition-colors"
          title={ciChecks.map(c => `${c.conclusion === "success" ? "✓" : c.conclusion === "failure" ? "✗" : "◌"} ${c.name}`).join("\n")}
          tabindex="-1"
          onclick={() => (ciExpanded = !ciExpanded)}
        >
          <span>{ciSummary()}</span>
          <ChevronDown class="size-3" />
        </button>
        <button
          class="ml-0.5 p-0.5 rounded text-surface-400 hover:text-surface-600 dark:hover:text-surface-300"
          tabindex="-1"
          title="Refresh checks"
          onclick={() => fetchCiChecks()}
        >
          <RefreshCw size={11} />
        </button>

        {#if ciExpanded}
          <div class="absolute top-full right-0 mt-1 z-50 w-64 rounded-lg border border-surface-200 dark:border-surface-700 bg-surface-50 dark:bg-surface-900 shadow-lg p-2">
            <div class="text-[10px] text-surface-500 uppercase tracking-wide mb-1">CI Checks</div>
            <ul class="space-y-0.5">
              {#each ciChecks as check (check.name)}
                {@const icon = check.conclusion === "success" || check.conclusion === "neutral" || check.conclusion === "skipped" ? "✓" : check.conclusion === "failure" || check.conclusion === "cancelled" || check.conclusion === "timed_out" ? "✗" : "◌"}
                {@const color = icon === "✓" ? "text-green-600 dark:text-green-400" : icon === "✗" ? "text-red-600 dark:text-red-400" : "text-yellow-500 animate-pulse"}
                <li class="flex items-center gap-1.5 text-xs">
                  <span class={color}>{icon}</span>
                  {#if check.url}
                    <button class="text-surface-700 dark:text-surface-300 hover:underline truncate text-left" onclick={() => openUrl(check.url!)}>{check.name}</button>
                  {:else}
                    <span class="text-surface-700 dark:text-surface-300 truncate">{check.name}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    {/if}
  {:else if hasChanges && onCreatePr}
    <button
      class="ml-2 shrink-0 flex items-center gap-1 px-2 py-1 rounded text-xs text-surface-600 dark:text-surface-400 hover:bg-surface-200 dark:hover:bg-surface-800 transition-colors"
      title="Create pull request"
      tabindex="-1"
      onmousedown={(e: MouseEvent) => e.preventDefault()}
      onclick={onCreatePr}
    >
      <GitPullRequest class="size-3.5" />
      <span>Create PR</span>
    </button>
  {/if}

  <div class="w-3 shrink-0"></div>
</header>
