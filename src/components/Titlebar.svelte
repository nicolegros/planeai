<script lang="ts">
  import { IS_MAC } from "../lib/keyboard";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { GitPullRequest, GitMerge, Zap, RefreshCw } from "@lucide/svelte";
  import { getCiChecks, classifyCheck, refreshCiChecks, type CiConclusion } from "../lib/ci-checks.svelte";
  import { pr, pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getActiveSession } from "../lib/session-orchestrator.svelte";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "../lib/session-tabs.svelte";

  interface Props {
    projectName: string | null;
    sessionName: string | null;
    sidebarVisible: boolean;
    tabs: Tab[];
    activeTabIndex: number;
    prUrl: string | null;
    prState: string | null;
    hasChanges: boolean;
    sessionId: string | null;
    symphonyStatus: { active: boolean; slots_used: number; max_concurrent: number } | null;
    runningCount: number;
    activeProvider: string | null;
    onSelectTab: (index: number) => void;
    onCloseTab: (index: number) => void;
    onAddTab: () => void;
    onCreatePr?: () => void;
  }

  let { projectName, sessionName, sidebarVisible, tabs, activeTabIndex, prUrl, prState, hasChanges, sessionId, symphonyStatus, runningCount, activeProvider, onSelectTab, onCloseTab, onAddTab, onCreatePr }: Props = $props();

  const platformPadding = IS_MAC ? "pl-20" : "pr-36";
  const STORAGE_KEY = "planeai:merge-strategy";

  let merging = $state(false);
  let allowedStrategies = $state<string[]>([]);
  let selectedStrategy = $state<string>(localStorage.getItem(STORAGE_KEY) || "squash");
  let strategiesFetchedFor = $state<string | null>(null);
  let prPanelOpen = $state(false);

  let checks = $derived(sessionId ? getCiChecks(sessionId) : []);
  let failedCount = $derived(checks.filter((c) => classifyCheck(c) === "fail").length);
  let allConcluded = $derived(checks.length > 0 && checks.every((c) => classifyCheck(c) !== "pending"));
  let checksPassing = $derived(checks.length === 0 || (allConcluded && failedCount === 0));
  let isMerged = $derived(prState === "merged");
  let isDraft = $derived(prState === "draft");
  let canMerge = $derived(prUrl && !isMerged && !isDraft && checksPassing && !merging);

  function iconFor(c: CiConclusion): { char: string; color: string } {
    if (c === "pass") return { char: "✓", color: "text-status-running" };
    if (c === "fail") return { char: "✗", color: "text-error-400" };
    return { char: "◌", color: "text-warning-400 animate-pulse" };
  }

  let sessionExited = $derived(getActiveSession()?.status === "exited");

  async function sendFailuresToAgent() {
    if (!sessionId || sessionExited) return;
    try {
      let msg: string;
      try {
        msg = await pr.getCiFailureLogs(sessionId);
      } catch {
        const lines = checks.map((c) => {
          const cls = classifyCheck(c);
          const icon = cls === "fail" ? "❌" : cls === "pass" ? "✓" : "◌";
          return `${icon} ${c.name} (${c.conclusion ?? "pending"})`;
        });
        msg = `CI checks failed for this PR. Please fix the following:\n\n${lines.join("\n")}`;
      }
      const bytes = Array.from(new TextEncoder().encode(msg + "\r"));
      await pty.write(sessionId, bytes);
      showSnackbar("CI failures sent to agent", "success");
    } catch (e: any) {
      showSnackbar(e.toString());
    }
  }

  function mergeDisabledReason(): string | null {
    if (isMerged) return "PR already merged";
    if (isDraft) return "PR is still a draft";
    if (merging) return "Merging…";
    if (failedCount > 0) return "CI checks failing";
    if (checks.length > 0 && !allConcluded) return "CI checks pending";
    return null;
  }

  async function fetchStrategies() {
    if (!sessionId || strategiesFetchedFor === sessionId) return;
    try {
      allowedStrategies = await pr.getAllowedStrategies(sessionId);
      strategiesFetchedFor = sessionId;
      if (allowedStrategies.length > 0 && !allowedStrategies.includes(selectedStrategy)) {
        selectedStrategy = allowedStrategies[0];
      }
    } catch {
      allowedStrategies = ["squash", "merge", "rebase"];
    }
  }

  async function doMerge(strategy: string) {
    if (!sessionId || !canMerge) return;
    merging = true;
    prPanelOpen = false;
    try {
      await pr.merge(sessionId, strategy);
      selectedStrategy = strategy;
      localStorage.setItem(STORAGE_KEY, strategy);
      showSnackbar("PR merged ✓", "success");
    } catch (e) {
      showSnackbar(String(e), "error");
    } finally {
      merging = false;
    }
  }

  async function markReady() {
    if (!sessionId) return;
    try {
      await pr.markReady(sessionId);
      showSnackbar("PR marked as ready", "success");
    } catch (e) {
      showSnackbar(String(e), "error");
    }
  }

  $effect(() => {
    if (prUrl && sessionId && !isMerged) {
      fetchStrategies();
    }
  });

  function handleClickOutside(e: MouseEvent) {
    if (prPanelOpen) prPanelOpen = false;
  }
</script>

<svelte:window onclick={handleClickOutside} />

<header
  data-tauri-drag-region
  class="h-[38px] flex items-center {platformPadding} shrink-0 bg-surface-100 dark:bg-surface-800 border-b border-border"
>
  {#if projectName || sessionName}
    <span class="text-[12.5px] select-none pointer-events-none whitespace-nowrap mr-3 flex items-center gap-1">
      {#if projectName}<span class="text-text-2">{projectName}</span>{/if}
      {#if projectName && sessionName}<span class="text-text-3 mx-0.5">/</span>{/if}
      {#if sessionName}<span class="text-text-1 font-medium">{sessionName}</span>{/if}
    </span>
    {#if activeProvider}
      <span class="font-mono text-[10px] px-1.5 py-0.5 rounded border border-border-strong text-text-3 mr-2 select-none">{activeProvider.toUpperCase()}</span>
    {/if}
    <span class="w-px h-4 bg-border-strong shrink-0 mr-2"></span>
  {/if}

  <div class="flex-1 min-w-0 h-full">
    <TabBar {tabs} {activeTabIndex} onSelect={onSelectTab} onClose={onCloseTab} onAdd={onAddTab} />
  </div>

  <!-- Right cluster -->
  <div class="flex items-center gap-2 ml-2 shrink-0">
    {#if runningCount > 0}
      <span class="flex items-center gap-1.5 text-[11px] text-status-running select-none">
        <span class="size-2 rounded-full bg-status-running animate-pulse"></span>
        <span class="font-medium">{runningCount} running</span>
      </span>
    {/if}

    {#if symphonyStatus?.active}
      <span
        class="flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] text-warning-400 bg-warning-50 dark:bg-warning-50/10"
        title="Orchestrator running"
      >
        <Zap class="size-3" />
        <span>{symphonyStatus.slots_used}/{symphonyStatus.max_concurrent}</span>
      </span>
    {/if}

    {#if prUrl}
      <div class="relative flex items-center" onclick={(e) => e.stopPropagation()}>
        <button
          class="flex items-center gap-1.5 px-2 py-1 rounded-md text-[11px] font-medium transition-colors
            {isMerged ? 'text-purple-500 bg-purple-500/10' : prState === 'draft' ? 'text-text-3 bg-surface-200 dark:bg-surface-600' : 'text-status-running bg-status-running/10'}"
          title="Pull request ({prState ?? 'open'})"
          tabindex="-1"
          onclick={() => (prPanelOpen = !prPanelOpen)}
        >
          {#if isMerged}<GitMerge class="size-3.5" />{:else}<GitPullRequest class="size-3.5" />{/if}
          <span>PR</span>
          {#if checks.length > 0}
            <span class="size-2 rounded-full {failedCount > 0 ? 'bg-error-400' : allConcluded ? 'bg-status-running' : 'bg-warning-400 animate-pulse'}"></span>
          {/if}
        </button>

        {#if prPanelOpen}
          {@const disabledReason = mergeDisabledReason()}
          <div class="absolute top-full right-0 mt-1.5 z-50 w-[282px] rounded-xl border border-border-strong bg-surface-50 dark:bg-surface-700 shadow-[0_18px_50px_-12px_rgba(0,0,0,0.45)] p-4 space-y-3">
            <!-- PR link -->
            <div class="flex items-center justify-between gap-2">
              <button class="text-xs text-primary-500 hover:underline font-medium truncate font-mono" onclick={() => openUrl(prUrl!)}>
                {sessionName ?? "Open PR"} ↗
              </button>
              <span class="text-[10px] text-text-3 shrink-0">{isMerged ? "merged" : prState ?? "open"}</span>
            </div>

            <!-- Mark as ready -->
            {#if isDraft}
              <button
                class="w-full px-2 py-1.5 text-xs rounded-lg border border-status-running/40 text-status-running hover:bg-status-running/10 font-medium"
                onclick={markReady}
              >
                Mark as ready
              </button>
            {/if}

            <!-- CI Checks -->
            {#if checks.length > 0}
              <div class="border-t border-border pt-3 space-y-1.5">
                <div class="flex items-center justify-between">
                  <span class="text-[10px] text-text-3 uppercase tracking-wide font-semibold">Checks</span>
                  <button
                    class="text-text-3 hover:text-text-2"
                    onclick={() => refreshCiChecks(sessionId!)}
                  >
                    <RefreshCw size={11} />
                  </button>
                </div>
                {#each checks as check, i (i)}
                  {@const ic = iconFor(classifyCheck(check))}
                  <div class="flex items-center gap-1.5 text-xs">
                    <span class={ic.color}>{ic.char}</span>
                    {#if check.url}
                      <button class="text-text-1 hover:underline truncate text-left" onclick={() => openUrl(check.url!)}>{check.name}</button>
                    {:else}
                      <span class="text-text-1 truncate">{check.name}</span>
                    {/if}
                  </div>
                {/each}
                <p class="text-[10px] text-text-3">{checks.filter(c => classifyCheck(c) === 'pass').length} passed</p>
                {#if failedCount > 0}
                  <button
                    class="mt-1 w-full text-xs px-2 py-1.5 rounded-lg bg-error-400/10 text-error-400 hover:bg-error-400/20 disabled:opacity-50 disabled:cursor-not-allowed"
                    disabled={sessionExited}
                    title={sessionExited ? "Agent is not running" : "Send failures to agent"}
                    onclick={sendFailuresToAgent}
                  >
                    Send failures to agent
                  </button>
                {/if}
              </div>
            {/if}

            <!-- Merge -->
            {#if !isMerged}
              <div class="border-t border-border pt-3">
                <div class="text-[10px] text-text-3 uppercase tracking-wide font-semibold mb-2">Merge</div>
                <div class="flex gap-0.5 rounded-lg bg-surface-200 dark:bg-surface-600 p-0.5">
                  {#each allowedStrategies as strat (strat)}
                    <button
                      class="flex-1 px-2 py-1.5 text-[11px] rounded-md capitalize transition-colors {strat === selectedStrategy ? 'bg-primary-500 text-white' : 'text-text-2 hover:text-text-1'}"
                      onclick={() => { selectedStrategy = strat; localStorage.setItem(STORAGE_KEY, strat); }}
                    >
                      {strat}
                    </button>
                  {/each}
                </div>
                <button
                  class="w-full mt-2 px-2 py-2 text-xs rounded-lg bg-primary-500 text-white hover:bg-primary-600 font-medium disabled:opacity-50 disabled:pointer-events-none"
                  disabled={!canMerge}
                  title={disabledReason ?? ""}
                  onclick={() => doMerge(selectedStrategy)}
                >
                  {merging ? "Merging…" : `Merge with ${selectedStrategy} ⌘↵`}
                </button>
                {#if disabledReason && !merging}
                  <p class="text-[10px] text-text-3 mt-1">{disabledReason}</p>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {:else if hasChanges && onCreatePr}
      <button
        class="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] text-text-2 border border-border-strong hover:bg-surface-200 dark:hover:bg-surface-600 transition-colors"
        title="Create pull request"
        tabindex="-1"
        onmousedown={(e: MouseEvent) => e.preventDefault()}
        onclick={onCreatePr}
      >
        <GitPullRequest class="size-3.5" />
        <span>Create PR</span>
      </button>
    {/if}

    <span class="font-mono text-[10px] px-1.5 py-0.5 rounded-md bg-surface-200 dark:bg-surface-600 text-text-3 select-none">⌘K</span>
  </div>

  <div class="w-2 shrink-0"></div>
</header>
