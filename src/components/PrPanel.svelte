<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { RefreshCw } from "@lucide/svelte";
  import { getCiChecks, classifyCheck, refreshCiChecks, type CiConclusion } from "../lib/ci-checks.svelte";
  import { pr, pty } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { getActiveSession } from "../lib/session-orchestrator.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";

  interface Props {
    sessionId: string;
    prUrl: string;
    prState: string | null;
    sessionName: string | null;
    onClose: () => void;
  }

  let { sessionId, prUrl, prState, sessionName, onClose }: Props = $props();

  const STORAGE_KEY = "planeai:merge-strategy";

  let merging = $state(false);
  let allowedStrategies = $state<string[]>([]);
  let selectedStrategy = $state<string>(localStorage.getItem(STORAGE_KEY) || "squash");
  let wrapperEl = $state<HTMLDivElement | null>(null);

  let checks = $derived(getCiChecks(sessionId));
  let failedCount = $derived(checks.filter((c) => classifyCheck(c) === "fail").length);
  let allConcluded = $derived(checks.length > 0 && checks.every((c) => classifyCheck(c) !== "pending"));
  let checksPassing = $derived(checks.length === 0 || (allConcluded && failedCount === 0));
  let isMerged = $derived(prState === "merged");
  let isDraft = $derived(prState === "draft");
  let canMerge = $derived(prUrl && !isMerged && !isDraft && checksPassing && !merging);
  let sessionExited = $derived(getActiveSession()?.status === "exited");

  $effect(() => { if (wrapperEl) wrapperEl.focus(); });
  $effect(() => { fetchStrategies(); });

  const fk = createFormKeyboardController(
    () => [
      { key: "o", toggle: () => openUrl(prUrl) },
      { key: "m", toggle: doMerge },
      { key: "r", toggle: markReady },
      { key: "f", toggle: sendFailuresToAgent },
      { key: "s", toggle: cycleStrategy },
      { key: "d", toggle: () => markReady() },
    ],
    { wrapper: () => wrapperEl, onDismiss: onClose },
  );

  const badge = $derived(fk.mode === "normal" ? "bg-accent-bg text-accent" : "bg-panel-hi text-t3");

  function iconFor(c: CiConclusion): { char: string; color: string } {
    if (c === "pass") return { char: "✓", color: "text-status-running" };
    if (c === "fail") return { char: "✗", color: "text-status-exited" };
    return { char: "◌", color: "text-status-review animate-pulse" };
  }

  async function fetchStrategies() {
    try {
      allowedStrategies = await pr.getAllowedStrategies(sessionId);
      if (allowedStrategies.length > 0 && !allowedStrategies.includes(selectedStrategy)) selectedStrategy = allowedStrategies[0];
    } catch { allowedStrategies = ["squash", "merge", "rebase"]; }
  }

  function cycleStrategy() {
    if (allowedStrategies.length === 0) return;
    const idx = allowedStrategies.indexOf(selectedStrategy);
    selectedStrategy = allowedStrategies[(idx + 1) % allowedStrategies.length];
    localStorage.setItem(STORAGE_KEY, selectedStrategy);
  }

  async function doMerge() {
    if (!canMerge) return;
    merging = true;
    try {
      await pr.merge(sessionId, selectedStrategy);
      localStorage.setItem(STORAGE_KEY, selectedStrategy);
      showSnackbar("PR merged ✓", "success");
      onClose();
    } catch (e) { showSnackbar(String(e), "error"); }
    finally { merging = false; }
  }

  async function markReady() {
    if (!isDraft) return;
    try { await pr.markReady(sessionId); showSnackbar("PR marked as ready", "success"); }
    catch (e) { showSnackbar(String(e), "error"); }
  }

  async function sendFailuresToAgent() {
    if (sessionExited || failedCount === 0) return;
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
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div bind:this={wrapperEl} tabindex="-1" onkeydown={fk.handleKeydown} onfocusin={fk.handleFocusin} class="outline-none px-5 pb-5" data-form-keyboard>
  <!-- Header -->
  <div class="flex items-center gap-2 pb-3 border-b border-border">
    <button class="text-[13px] font-medium text-accent truncate font-mono" onclick={() => openUrl(prUrl)}>{sessionName ?? "PR"} ↗</button>
    <span class="ml-auto text-[10px] tracking-[.06em] uppercase font-mono {isMerged ? 'text-[#bc8cff]' : 'text-status-running'}">{isMerged ? 'merged' : prState ?? 'open'}</span>
  </div>

  <!-- Draft -->
  {#if isDraft}
    <div class="py-3 border-b border-border">
      <button class="w-full py-1.5 text-xs rounded-lg border border-status-running/40 text-status-running hover:bg-status-running/10 font-medium" onclick={markReady}>
        Mark as ready <span class="font-mono text-[10px] px-1 rounded {badge}">R</span>
      </button>
    </div>
  {/if}

  <!-- Checks -->
  {#if checks.length > 0}
    <div class="py-3 border-b border-border">
      <div class="flex items-center mb-2">
        <span class="text-[10px] font-semibold tracking-[.06em] uppercase text-t3">Checks</span>
        <button class="ml-auto text-t3 hover:text-t2" onclick={() => refreshCiChecks(sessionId)}><RefreshCw size={11} /></button>
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
        <button class="mt-2 w-full text-xs px-2 py-1.5 rounded-lg bg-status-exited/10 text-status-exited hover:bg-status-exited/20 disabled:opacity-50" disabled={sessionExited} onclick={sendFailuresToAgent}>
          Send failures to agent <span class="font-mono text-[10px] px-1 rounded {badge}">F</span>
        </button>
      {/if}
    </div>
  {/if}

  <!-- Merge -->
  {#if !isMerged}
    <div class="py-3">
      <div class="text-[10px] font-semibold tracking-[.06em] uppercase text-t3 mb-2">Merge</div>
      <div class="flex gap-1.5 mb-2">
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
        onclick={doMerge}
      >
        {merging ? "Merging…" : `Merge with ${selectedStrategy}`} <span class="font-mono text-[10px] px-1 rounded {badge}">M</span>
      </button>
    </div>
  {/if}

  <!-- Footer with mode indicator and key hints -->
  <div class="flex items-center justify-between pt-2 border-t border-border">
    <div class="flex items-center gap-2">
      {#if fk.mode === "normal"}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
      {/if}
    </div>
    <div class="flex gap-3 text-[10px] text-t3 font-mono">
      <span><span class="px-1 rounded {badge}">O</span> open</span>
      <span><span class="px-1 rounded {badge}">S</span> strategy</span>
      {#if isDraft}<span><span class="px-1 rounded {badge}">R</span> ready</span>{/if}
      {#if failedCount > 0}<span><span class="px-1 rounded {badge}">F</span> failures</span>{/if}
    </div>
  </div>
</div>
