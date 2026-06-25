<script lang="ts">
  import { IS_MAC, MOD_LABEL, MOD_ENTER_HINT } from "../lib/keyboard";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { GitPullRequest, GitMerge, RefreshCw, Bot, Terminal, GitCompare, FileCode } from "@lucide/svelte";
  import { getCiChecks, classifyCheck, refreshCiChecks, type CiConclusion } from "../lib/ci-checks.svelte";
  import { pr, pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getActiveSession } from "../lib/session-orchestrator.svelte";
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
    onOpenCommand?: () => void;
  }

  let { projectName, sessionName, sidebarVisible, tabs, activeTabIndex, prUrl, prState, hasChanges, sessionId, symphonyStatus, runningCount, activeProvider, onSelectTab, onCloseTab, onAddTab, onCreatePr, onOpenCommand }: Props = $props();

  const platformPadding = IS_MAC ? "pl-[72px]" : "pr-36";
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

  const TAB_ICONS: Record<string, typeof Bot> = { bot: Bot, "git-compare": GitCompare, file: FileCode, terminal: Terminal };

  function iconFor(c: CiConclusion): { char: string; color: string } {
    if (c === "pass") return { char: "✓", color: "text-status-running" };
    if (c === "fail") return { char: "✗", color: "text-status-exited" };
    return { char: "◌", color: "text-status-review animate-pulse" };
  }

  let sessionExited = $derived(getActiveSession()?.status === "exited");

  async function sendFailuresToAgent() {
    if (!sessionId || sessionExited) return;
    try {
      let msg: string;
      try { msg = await pr.getCiFailureLogs(sessionId); } catch {
        const lines = checks.map((c) => {
          const cls = classifyCheck(c);
          const icon = cls === "fail" ? "❌" : cls === "pass" ? "✓" : "◌";
          return `${icon} ${c.name} (${c.conclusion ?? "pending"})`;
        });
        msg = `CI checks failed for this PR. Please fix the following:\n\n${lines.join("\n")}`;
      }
      await pty.write(sessionId, Array.from(new TextEncoder().encode(msg + "\r")));
      showSnackbar("CI failures sent to agent", "success");
    } catch (e: any) { showSnackbar(e.toString()); }
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
      if (allowedStrategies.length > 0 && !allowedStrategies.includes(selectedStrategy)) selectedStrategy = allowedStrategies[0];
    } catch { allowedStrategies = ["squash", "merge", "rebase"]; }
  }

  async function doMerge(strategy: string) {
    if (!sessionId || !canMerge) return;
    merging = true; prPanelOpen = false;
    try {
      await pr.merge(sessionId, strategy);
      selectedStrategy = strategy; localStorage.setItem(STORAGE_KEY, strategy);
      showSnackbar("PR merged ✓", "success");
    } catch (e) { showSnackbar(String(e), "error"); }
    finally { merging = false; }
  }

  async function markReady() {
    if (!sessionId) return;
    try { await pr.markReady(sessionId); showSnackbar("PR marked as ready", "success"); }
    catch (e) { showSnackbar(String(e), "error"); }
  }

  $effect(() => { if (prUrl && sessionId && !isMerged) fetchStrategies(); });

  function handleClickOutside() { if (prPanelOpen) prPanelOpen = false; }
</script>

<svelte:window onclick={handleClickOutside} />

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
        <Icon size={13} class="{tab.index === activeTabIndex ? 'text-accent' : 'text-t3'}" />
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
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div class="relative flex items-center" role="presentation" onclick={(e) => e.stopPropagation()}>
        <button
          class="flex items-center gap-[7px] h-[25px] px-[9px] rounded-[7px] text-[11.5px] font-medium
            {isMerged ? 'bg-[rgba(188,140,255,0.18)] text-[#bc8cff]' : isDraft ? 'bg-panel-hi text-t2' : prState === 'closed' ? 'bg-status-exited/15 text-status-exited' : 'bg-[rgba(63,185,80,0.18)] text-status-running'}"
          onclick={() => (prPanelOpen = !prPanelOpen)}
        >
          <span class="size-[7px] rounded-full {isMerged ? 'bg-[#bc8cff]' : isDraft ? 'bg-t3' : prState === 'closed' ? 'bg-status-exited' : 'bg-status-running'}"></span>
          PR{prUrl?.match(/#(\d+)/)?.[0] ?? ''}
          {#if checks.length > 0}
            <span class="size-[5px] rounded-full {failedCount > 0 ? 'bg-status-exited' : allConcluded ? 'bg-status-running' : 'bg-status-review animate-pulse'}"></span>
          {/if}
        </button>

        {#if prPanelOpen}
          {@const disabledReason = mergeDisabledReason()}
          <div class="absolute top-[31px] right-0 z-[70] w-[282px] rounded-xl border border-border-s bg-panel shadow-[0_20px_54px_-12px_rgba(0,0,0,0.55)] overflow-hidden">
            <!-- Header -->
            <div class="flex items-center gap-2 px-[13px] py-3 border-b border-border">
              <button class="text-[13px] font-medium text-accent truncate font-mono" onclick={() => openUrl(prUrl!)}>{sessionName ?? "PR"} ↗</button>
              <span class="ml-auto text-[10px] tracking-[.06em] uppercase font-mono {isMerged ? 'text-[#bc8cff]' : 'text-status-running'}">{isMerged ? 'merged' : prState ?? 'open'}</span>
            </div>
            <!-- Draft -->
            {#if isDraft}
              <div class="px-[13px] py-[11px] border-b border-border">
                <button class="w-full py-1.5 text-xs rounded-lg border border-status-running/40 text-status-running hover:bg-status-running/10 font-medium" onclick={markReady}>Mark as ready</button>
              </div>
            {/if}
            <!-- Checks -->
            {#if checks.length > 0}
              <div class="px-[13px] py-[11px] border-b border-border">
                <div class="flex items-center mb-[9px]">
                  <span class="text-[10px] font-semibold tracking-[.06em] uppercase text-t3">Checks</span>
                  <button class="ml-auto text-t3 hover:text-t2" onclick={() => refreshCiChecks(sessionId!)}><RefreshCw size={11} /></button>
                  <span class="ml-2 font-mono text-[10px] text-status-running">{checks.filter(c => classifyCheck(c) === 'pass').length} passed</span>
                </div>
                <div class="flex flex-col gap-1.5">
                  {#each checks as check, i (i)}
                    {@const ic = iconFor(classifyCheck(check))}
                    {#if check.url}
                      <button class="flex items-center gap-2 hover:bg-panel-hi rounded px-1 -mx-1 text-left" onclick={() => openUrl(check.url!)}>
                        <span class="w-[14px] text-center {ic.color}">{ic.char}</span>
                        <span class="font-mono text-[11.5px] text-t2 truncate">{check.name}</span>
                      </button>
                    {:else}
                      <div class="flex items-center gap-2">
                        <span class="w-[14px] text-center {ic.color}">{ic.char}</span>
                        <span class="font-mono text-[11.5px] text-t2 truncate">{check.name}</span>
                      </div>
                    {/if}
                  {/each}
                </div>
                {#if failedCount > 0}
                  <button class="mt-2 w-full text-xs px-2 py-1.5 rounded-lg bg-status-exited/10 text-status-exited hover:bg-status-exited/20 disabled:opacity-50" disabled={sessionExited} onclick={sendFailuresToAgent}>Send failures to agent</button>
                {/if}
              </div>
            {/if}
            <!-- Merge -->
            {#if !isMerged}
              <div class="px-[13px] py-[11px]">
                <div class="text-[10px] font-semibold tracking-[.06em] uppercase text-t3 mb-2">Merge</div>
                <div class="flex gap-1.5 mb-[9px]">
                  {#each allowedStrategies as strat (strat)}
                    <button
                      class="flex-1 text-center py-1.5 rounded-[7px] text-[11.5px] font-medium border
                        {strat === selectedStrategy ? 'bg-accent text-on-accent border-accent' : 'bg-panel-hi text-t2 border-border'}"
                      onclick={() => { selectedStrategy = strat; localStorage.setItem(STORAGE_KEY, strat); }}
                    >{strat}</button>
                  {/each}
                </div>
                <button
                  class="w-full flex items-center justify-center gap-2 h-[34px] rounded-lg bg-accent text-on-accent text-[12.5px] font-medium disabled:opacity-50 disabled:pointer-events-none"
                  disabled={!canMerge}
                  title={disabledReason ?? ""}
                  onclick={() => doMerge(selectedStrategy)}
                >
                  {merging ? "Merging…" : `Merge with ${selectedStrategy}`}<span class="font-mono text-[10px] opacity-80">{MOD_ENTER_HINT}</span>
                </button>
                {#if disabledReason && !merging}<p class="text-[10px] text-t3 mt-1">{disabledReason}</p>{/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
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
