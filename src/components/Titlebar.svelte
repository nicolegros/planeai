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
    onSelectTab: (index: number) => void;
    onCloseTab: (index: number) => void;
    onAddTab: () => void;
    onCreatePr?: () => void;
  }

  let { projectName, sessionName, sidebarVisible, tabs, activeTabIndex, prUrl, prState, hasChanges, sessionId, symphonyStatus, onSelectTab, onCloseTab, onAddTab, onCreatePr }: Props = $props();

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
    if (c === "pass") return { char: "✓", color: "text-green-600 dark:text-green-400" };
    if (c === "fail") return { char: "✗", color: "text-red-600 dark:text-red-400" };
    return { char: "◌", color: "text-yellow-500 animate-pulse" };
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
    <div class="ml-2 shrink-0 relative flex items-center" onclick={(e) => e.stopPropagation()}>
      <button
        class="relative flex items-center gap-1.5 px-2 py-1 rounded text-xs hover:bg-surface-200 dark:hover:bg-surface-800 transition-colors {isMerged ? 'text-purple-600 dark:text-purple-400' : prState === 'draft' ? 'text-surface-500 dark:text-surface-400' : 'text-green-600 dark:text-green-400'}"
        title="Pull request ({prState ?? 'open'})"
        tabindex="-1"
        onclick={() => (prPanelOpen = !prPanelOpen)}
      >
        {#if isMerged}<GitMerge class="size-3.5" />{:else}<GitPullRequest class="size-3.5" />{/if}
        {#if checks.length > 0}
          <span class="absolute top-0.5 right-0.5 size-2 rounded-full border border-surface-100 dark:border-surface-900 {failedCount > 0 ? 'bg-red-500' : allConcluded ? 'bg-green-500' : 'bg-yellow-500 animate-pulse'}"></span>
        {/if}
      </button>

      {#if prPanelOpen}
        {@const disabledReason = mergeDisabledReason()}
        <div class="absolute top-full right-0 mt-1 z-50 w-72 rounded-xl border border-surface-200 dark:border-surface-700 bg-surface-50 dark:bg-surface-900 shadow-xl p-4 space-y-3">
          <!-- PR link -->
          <div class="flex items-center justify-between gap-2">
            <button class="text-xs text-primary-600 dark:text-primary-400 hover:underline font-medium truncate" onclick={() => openUrl(prUrl!)}>
              {sessionName ?? "Open PR"} ↗
            </button>
            <span class="text-[10px] text-surface-400 shrink-0">{isMerged ? "merged" : prState ?? "open"}</span>
          </div>

          <!-- Mark as ready -->
          {#if isDraft}
            <button
              class="w-full px-2 py-1.5 text-xs rounded border border-green-300 dark:border-green-700 text-green-700 dark:text-green-400 hover:bg-green-50 dark:hover:bg-green-950/30 font-medium"
              onclick={markReady}
            >
              Mark as ready
            </button>
          {/if}

          <!-- CI Checks -->
          {#if checks.length > 0}
            <div class="border-t border-surface-200 dark:border-surface-700 pt-2 space-y-1">
              <div class="flex items-center justify-between">
                <span class="text-[10px] text-surface-500 uppercase tracking-wide">Checks</span>
                <button
                  class="text-[10px] text-surface-400 hover:text-surface-600 dark:hover:text-surface-300"
                  onclick={() => refreshCiChecks(sessionId!)}
                >
                  <RefreshCw size={10} />
                </button>
              </div>
              {#each checks as check, i (i)}
                {@const ic = iconFor(classifyCheck(check))}
                <div class="flex items-center gap-1.5 text-xs">
                  <span class={ic.color}>{ic.char}</span>
                  {#if check.url}
                    <button class="text-surface-700 dark:text-surface-300 hover:underline truncate text-left" onclick={() => openUrl(check.url!)}>{check.name}</button>
                  {:else}
                    <span class="text-surface-700 dark:text-surface-300 truncate">{check.name}</span>
                  {/if}
                </div>
              {/each}
              {#if failedCount > 0}
                <button
                  class="mt-2 w-full text-xs px-2 py-1 rounded bg-red-50 dark:bg-red-950/30 text-red-700 dark:text-red-300 hover:bg-red-100 dark:hover:bg-red-900/40 disabled:opacity-50 disabled:cursor-not-allowed"
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
            <div class="border-t border-surface-200 dark:border-surface-700 pt-2">
              <div class="text-[10px] text-surface-500 uppercase tracking-wide mb-1.5">Merge</div>
              <div class="flex gap-1">
                {#each allowedStrategies as strat (strat)}
                  <button
                    class="flex-1 px-2 py-1.5 text-xs rounded capitalize {strat === selectedStrategy ? 'bg-purple-600 text-white' : 'bg-surface-200 dark:bg-surface-700 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-600'}"
                    onclick={() => { selectedStrategy = strat; localStorage.setItem(STORAGE_KEY, strat); }}
                  >
                    {strat}
                  </button>
                {/each}
              </div>
              <button
                class="w-full mt-2 px-2 py-1.5 text-xs rounded bg-purple-600 text-white hover:bg-purple-700 font-medium disabled:opacity-50 disabled:pointer-events-none"
                disabled={!canMerge}
                title={disabledReason ?? ""}
                onclick={() => doMerge(selectedStrategy)}
              >
                {merging ? "Merging…" : `Merge with ${selectedStrategy}`}
              </button>
              {#if disabledReason && !merging}
                <p class="text-[10px] text-surface-400 mt-1">{disabledReason}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>
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
