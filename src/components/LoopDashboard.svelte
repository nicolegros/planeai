<script lang="ts">
  import type { LoopRunDetail } from "../lib/types";
  import { loops as loopsApi, git } from "../lib/api";
  import { Button } from "./ui";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { RefreshCw, Play, Square, ExternalLink, Copy, CheckCircle2, XCircle, Clock, ChevronRight, ChevronDown } from "@lucide/svelte";
  import LoopTimeline from "./LoopTimeline.svelte";

  interface Props {
    loopId: string;
    projectPath: string;
    onSelectSession: (sessionId: string) => void;
    onOpenArtifact?: (path: string) => void;
  }

  let { loopId, projectPath, onSelectSession, onOpenArtifact }: Props = $props();

  let detail = $state<LoopRunDetail | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Verifier output expansion state
  let expandedVerifiers = $state<Set<string>>(new Set());
  let verifierOutputs = $state<Record<string, { content: string | null; loading: boolean; error: string | null }>>({});

  // eslint-disable-next-line no-control-regex -- intentional: stripping ANSI escape sequences
  const ANSI_REGEX = /\x1b\[[0-9;]*[A-Za-z]|\x1b\][^\x07]*\x07|\r/g;

  function stripAnsi(text: string): string {
    return text.replace(ANSI_REGEX, "");
  }

  async function toggleVerifierOutput(vrId: string, outputPath: string | null) {
    if (expandedVerifiers.has(vrId)) {
      expandedVerifiers = new Set([...expandedVerifiers].filter(id => id !== vrId));
      return;
    }
    expandedVerifiers = new Set([...expandedVerifiers, vrId]);

    if (verifierOutputs[vrId]?.content != null) return; // already loaded
    if (!outputPath) {
      verifierOutputs = { ...verifierOutputs, [vrId]: { content: null, loading: false, error: "No output file" } };
      return;
    }

    verifierOutputs = { ...verifierOutputs, [vrId]: { content: null, loading: true, error: null } };
    try {
      const raw = await git.readFile(outputPath, projectPath);
      verifierOutputs = { ...verifierOutputs, [vrId]: { content: stripAnsi(raw), loading: false, error: null } };
    } catch (e) {
      verifierOutputs = { ...verifierOutputs, [vrId]: { content: null, loading: false, error: String(e) } };
    }
  }

  async function refresh() {
    loading = true;
    error = null;
    try {
      detail = await loopsApi.detail(loopId);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Load on mount and when loopId changes
  $effect(() => {
    refresh();
  });

  async function handleTick() {
    try {
      await loopsApi.tick(loopId);
      refresh();
    } catch (e) {
      showSnackbar(`Tick failed: ${e}`);
    }
  }

  async function handleStop() {
    try {
      await loopsApi.stop(loopId);
      refresh();
    } catch (e) {
      showSnackbar(`Stop failed: ${e}`);
    }
  }

  async function handleStart() {
    try {
      await loopsApi.start(loopId);
      refresh();
    } catch (e) {
      showSnackbar(`Start failed: ${e}`);
    }
  }

  function copyPath(path: string) {
    navigator.clipboard.writeText(path);
  }

  function isActive(status: string): boolean {
    return ["running", "observing", "verifying"].includes(status);
  }

  function shortId(id: string): string {
    return id.slice(0, 8);
  }

  // ─── Keyboard shortcuts ──────────────────────────────────────────────────────

  function handleKeydown(e: KeyboardEvent) {
    // Don't intercept when focus is in an input or when modifier keys are held
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const el = document.activeElement;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT")) return;

    if (e.key === "r") {
      e.preventDefault();
      refresh();
    } else if (e.key === "s" && detail?.run.status === "draft") {
      e.preventDefault();
      handleStart();
    } else if (e.key === "t" && detail && isActive(detail.run.status)) {
      e.preventDefault();
      handleTick();
    } else if (e.key === "x" && detail && isActive(detail.run.status)) {
      e.preventDefault();
      handleStop();
    } else if (e.key >= "1" && e.key <= "9" && detail) {
      // Number keys open sessions (1-indexed)
      const idx = parseInt(e.key) - 1;
      if (idx < detail.sessions.length) {
        e.preventDefault();
        onSelectSession(detail.sessions[idx].session_id);
      }
    }
  }

  const statusBadgeColors: Record<string, string> = {
    draft: "bg-t3/20 text-t2",
    running: "bg-status-running/20 text-status-running",
    observing: "bg-status-running/20 text-status-running",
    verifying: "bg-status-running/20 text-status-running",
    completed_unreviewed: "bg-status-review/20 text-status-review",
    blocked: "bg-status-exited/20 text-status-exited",
    needs_human: "bg-status-review/20 text-status-review",
    stale: "bg-status-exited/20 text-status-exited",
    failed: "bg-status-exited/20 text-status-exited",
    cancelled: "bg-status-exited/20 text-status-exited",
    approved: "bg-status-running/20 text-status-running",
    merged: "bg-status-idle/20 text-status-idle",
    cleaned: "bg-status-idle/20 text-status-idle",
  };

  function verifierIcon(status: string) {
    if (status === "passed") return CheckCircle2;
    if (status === "failed") return XCircle;
    return Clock;
  }

  function verifierColor(status: string): string {
    if (status === "passed") return "text-status-running";
    if (status === "failed") return "text-status-exited";
    return "text-t3";
  }

  const hintBadge = "font-mono text-[10px] px-1 rounded bg-panel-hi text-t3";
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="h-full overflow-y-auto p-6 space-y-6">
  {#if loading && !detail}
    <div class="flex items-center justify-center py-12">
      <RefreshCw class="size-5 text-t3 animate-spin" />
    </div>
  {:else if error}
    <div class="text-status-exited text-sm p-4 rounded-md bg-status-exited/10">
      {error}
    </div>
  {:else if detail}
    <!-- Header -->
    <div class="space-y-2">
      <div class="flex items-center gap-3">
        <h1 class="text-xl font-semibold text-t1">
          {#if detail.run.task_key}
            {detail.run.task_key}
          {:else}
            Loop {shortId(detail.run.id)}
          {/if}
        </h1>
        <span class="px-2 py-0.5 rounded-full text-xs font-medium {statusBadgeColors[detail.run.status] ?? 'bg-t3/20 text-t2'}">
          {detail.run.status}
        </span>
        <span class="text-t3 text-sm">
          max {detail.run.max_rounds} rounds
        </span>
        <span class="text-t3 text-xs font-mono">{detail.run.strategy}</span>
      </div>

      <p class="text-t2 text-sm">{detail.run.goal}</p>

      <!-- Actions -->
      <div class="flex gap-2 pt-1">
        <Button variant="ghost" size="sm" class="gap-1.5" onclick={refresh} disabled={loading}>
          <RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
          Refresh
          <span class={hintBadge}>R</span>
        </Button>
        {#if detail.run.status === "draft"}
          <Button variant="primary" size="sm" class="gap-1.5" onclick={handleStart}>
            <Play class="size-3.5" />
            Start
            <span class={hintBadge}>S</span>
          </Button>
        {/if}
        {#if isActive(detail.run.status)}
          <Button variant="ghost" size="sm" class="gap-1.5" onclick={handleTick}>
            <Play class="size-3.5" />
            Tick
            <span class={hintBadge}>T</span>
          </Button>
          <Button variant="ghost" size="sm" class="gap-1.5" onclick={handleStop}>
            <Square class="size-3.5" />
            Stop
            <span class={hintBadge}>X</span>
          </Button>
        {/if}
      </div>
    </div>

    <!-- Main body: LoopTimeline for recipe loops, flat sections for non-recipe -->
    {#if detail.recipe_snapshot}
      <LoopTimeline
        snapshot={detail.recipe_snapshot}
        sessions={detail.sessions}
        verifierRuns={detail.verifier_runs}
        events={detail.events}
        artifacts={detail.artifacts}
        onSelectSession={onSelectSession}
        onOpenFile={onOpenArtifact}
        onLoadOutput={(path) => git.readFile(path, projectPath)}
      />
    {:else}
      <!-- Flat sections for non-recipe loops -->
      {#if detail.sessions.length > 0}
        <section>
          <h2 class="text-sm font-semibold text-t2 mb-2">Sessions</h2>
          <div class="border border-border rounded-md overflow-hidden">
            <table class="w-full text-sm">
              <thead class="bg-panel-hi text-t3 text-xs">
                <tr>
                  <th class="text-left px-3 py-1.5">#</th>
                  <th class="text-left px-3 py-1.5">Role</th>
                  <th class="text-left px-3 py-1.5">Session</th>
                  <th class="text-left px-3 py-1.5">Round</th>
                  <th class="text-left px-3 py-1.5">Provider</th>
                  <th class="text-left px-3 py-1.5">Status</th>
                  <th class="px-3 py-1.5"></th>
                </tr>
              </thead>
              <tbody>
                {#each detail.sessions as session, i (session.session_id)}
                  <tr class="border-t border-border hover:bg-panel-hi/50">
                    <td class="px-3 py-2 font-mono text-xs text-t3">{i + 1}</td>
                    <td class="px-3 py-2 text-t2 font-medium">{session.role}</td>
                    <td class="px-3 py-2 font-mono text-xs text-t3">{shortId(session.session_id)}</td>
                    <td class="px-3 py-2 text-t3">{session.round}</td>
                    <td class="px-3 py-2 text-t3">{session.provider ?? "—"}</td>
                    <td class="px-3 py-2 text-t2">{session.status}</td>
                    <td class="px-3 py-2">
                      <button class="text-accent hover:underline text-xs" onclick={() => onSelectSession(session.session_id)}>
                        Open <span class={hintBadge}>{i + 1}</span>
                      </button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </section>
      {/if}

      {#if detail.verifier_runs.length > 0}
        <section>
          <h2 class="text-sm font-semibold text-t2 mb-2">Verifier Runs</h2>
          <ul class="space-y-1">
            {#each detail.verifier_runs as vr (vr.id)}
              {@const Icon = verifierIcon(vr.status)}
              {@const isExpanded = expandedVerifiers.has(vr.id)}
              {@const output = verifierOutputs[vr.id]}
              <li class="rounded-md border border-border overflow-hidden">
                <button class="w-full flex items-center gap-2 px-3 py-2 hover:bg-panel-hi/50 transition-colors" onclick={() => toggleVerifierOutput(vr.id, vr.output_path)}>
                  {#if isExpanded}<ChevronDown class="size-3 text-t3 shrink-0" />{:else}<ChevronRight class="size-3 text-t3 shrink-0" />{/if}
                  <Icon class="size-4 {verifierColor(vr.status)}" />
                  <span class="text-t1 text-sm font-medium flex-1 text-left">{vr.name}</span>
                  <code class="text-t3 text-xs font-mono truncate max-w-[200px]">{vr.command}</code>
                  {#if vr.exit_code != null}
                    <span class="text-xs {vr.exit_code === 0 ? 'text-status-running' : 'text-status-exited'}">exit {vr.exit_code}</span>
                  {/if}
                </button>
                {#if isExpanded}
                  <div class="border-t border-border bg-panel">
                    {#if output?.loading}
                      <div class="px-3 py-2 text-xs text-t3">Loading…</div>
                    {:else if output?.error}
                      <div class="px-3 py-2 text-xs text-status-exited">{output.error}</div>
                    {:else if output?.content != null}
                      <pre class="px-3 py-2 text-xs font-mono text-t2 overflow-auto max-h-[400px] whitespace-pre-wrap break-words">{output.content}</pre>
                    {:else}
                      <div class="px-3 py-2 text-xs text-t3">No output available</div>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if detail.events.length > 0}
        <section>
          <h2 class="text-sm font-semibold text-t2 mb-2">Events</h2>
          <ul class="space-y-0.5 text-xs">
            {#each detail.events.slice(-20) as event (event.id)}
              {@const payload = event.payload_json as Record<string, unknown> | null}
              {@const stepId = payload?.step_id as string | undefined}
              <li class="flex items-center gap-2 px-2 py-1 rounded hover:bg-panel-hi/50">
                <span class="text-t2 font-medium min-w-[160px]">{event.kind}</span>
                {#if stepId}
                  <code class="text-t3 font-mono">{stepId}</code>
                {/if}
                <span class="ml-auto text-t3 font-mono shrink-0">{new Date(event.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false })}</span>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}
  {/if}
</div>
