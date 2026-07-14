<script lang="ts">
  import type { RecipeSnapshot, LoopSessionItem, LoopEventItem, VerifierRunItem, LoopArtifactItem } from "../lib/types";
  import { ChevronRight, ChevronDown, ExternalLink, FileText, Wrench, BookOpen, Shield, CheckCircle2, XCircle, Clock, Play } from "@lucide/svelte";

  // ─── Props ─────────────────────────────────────────────────────────────────────

  interface Props {
    snapshot: RecipeSnapshot;
    sessions: LoopSessionItem[];
    verifierRuns: VerifierRunItem[];
    events: LoopEventItem[];
    artifacts: LoopArtifactItem[];
    onSelectSession?: (sessionId: string) => void;
    onOpenFile?: (path: string) => void;
    onLoadOutput?: (path: string) => Promise<string>;
  }

  let { snapshot, sessions, verifierRuns, events, artifacts, onSelectSession, onOpenFile, onLoadOutput }: Props = $props();

  // ─── State ─────────────────────────────────────────────────────────────────────

  let configExpanded = $state(false);
  let expandedSteps = $state<Set<string>>(new Set());
  let lastAutoExpanded = $state("");
  let verifierOutputs = $state<Record<string, { content: string | null; loading: boolean; error: string | null }>>({});

  function toggleStep(stepId: string) {
    if (expandedSteps.has(stepId)) {
      expandedSteps = new Set([...expandedSteps].filter(id => id !== stepId));
    } else {
      expandedSteps = new Set([...expandedSteps, stepId]);
    }
  }

  async function loadVerifierOutput(vrId: string, outputPath: string | null) {
    if (verifierOutputs[vrId]?.content != null || verifierOutputs[vrId]?.loading) return;
    if (!outputPath || !onLoadOutput) {
      verifierOutputs = { ...verifierOutputs, [vrId]: { content: null, loading: false, error: "No output file" } };
      return;
    }
    verifierOutputs = { ...verifierOutputs, [vrId]: { content: null, loading: true, error: null } };
    try {
      const raw = await onLoadOutput(outputPath);
      // eslint-disable-next-line no-control-regex
      const content = raw.replace(/\x1b\[[0-9;]*[A-Za-z]|\x1b\][^\x07]*\x07|\r/g, "");
      verifierOutputs = { ...verifierOutputs, [vrId]: { content, loading: false, error: null } };
    } catch (e) {
      verifierOutputs = { ...verifierOutputs, [vrId]: { content: null, loading: false, error: String(e) } };
    }
  }

  // ─── Data Linkage (derived) ────────────────────────────────────────────────────

  // Index sessions by step: use created_session_ids from runtime when available, fallback to role
  const sessionsByStep = $derived.by(() => {
    const map: Record<string, LoopSessionItem[]> = {};
    const sessionMap: Record<string, LoopSessionItem> = {};
    for (const s of sessions) sessionMap[s.session_id] = s;

    // Primary: link via runtime.created_session_ids (role → session_id[])
    const createdIds = snapshot.runtime.created_session_ids ?? {};
    const assignedSessionIds = new Set<string>();
    for (const step of snapshot.steps) {
      if (!step.role) continue;
      const roleSessionIds = createdIds[step.role] ?? [];
      const stepSessions: LoopSessionItem[] = [];
      for (const sid of roleSessionIds) {
        const s = sessionMap[sid];
        if (s) {
          stepSessions.push(s);
          assignedSessionIds.add(sid);
        }
      }
      if (stepSessions.length > 0 && (step.kind === "session.create" || step.kind === "session.prompt")) {
        map[step.id] = stepSessions;
      }
    }

    // Fallback: sessions not assigned via created_session_ids → attach to first session.create with matching role
    for (const s of sessions) {
      if (assignedSessionIds.has(s.session_id)) continue;
      const step = snapshot.steps.find(st => st.role === s.role && st.kind === "session.create");
      if (step) (map[step.id] ??= []).push(s);
    }
    return map;
  });

  // Index verifier runs by role (for gates.run steps) + collect orphans
  const verifierData = $derived.by(() => {
    const sessionRoleMap: Record<string, string> = {};
    for (const s of sessions) sessionRoleMap[s.session_id] = s.role;
    const byRole: Record<string, VerifierRunItem[]> = {};
    const orphans: VerifierRunItem[] = [];
    for (const vr of verifierRuns) {
      const role = vr.session_id ? sessionRoleMap[vr.session_id] : undefined;
      if (role) (byRole[role] ??= []).push(vr);
      else orphans.push(vr);
    }
    return { byRole, orphans };
  });

  // Index events by step_id with safe payload parsing
  const eventsByStep = $derived.by(() => {
    const map: Record<string, LoopEventItem[]> = {};
    const orphans: LoopEventItem[] = [];
    for (const e of events) {
      const payload = (typeof e.payload_json === "object" && e.payload_json !== null && !Array.isArray(e.payload_json))
        ? (e.payload_json as Record<string, unknown>)
        : null;
      const stepId = payload?.step_id as string | undefined;
      if (stepId) (map[stepId] ??= []).push(e);
      else orphans.push(e);
    }
    return { map, orphans };
  });

  // Step statuses — linear approximation (branching not tracked)
  const stepStatuses = $derived.by(() => {
    const statuses: Record<string, "done" | "active" | "pending"> = {};
    const currentIdx = snapshot.steps.findIndex(s => s.id === snapshot.runtime.current_step);
    for (let i = 0; i < snapshot.steps.length; i++) {
      if (currentIdx === -1) {
        // current_step not found (loop finished or unknown state) — mark all done
        statuses[snapshot.steps[i].id] = "done";
      } else if (i < currentIdx) {
        statuses[snapshot.steps[i].id] = "done";
      } else if (i === currentIdx) {
        statuses[snapshot.steps[i].id] = "active";
      } else {
        statuses[snapshot.steps[i].id] = "pending";
      }
    }
    return statuses;
  });

  // Auto-expand active step only when it changes
  $effect(() => {
    const cs = snapshot.runtime.current_step;
    if (cs && cs !== lastAutoExpanded) {
      lastAutoExpanded = cs;
      expandedSteps = new Set([...expandedSteps, cs]);
    }
  });

  // ─── Helpers ───────────────────────────────────────────────────────────────────

  const sourceBadgeColors: Record<string, string> = {
    builtin: "bg-accent/20 text-accent",
    project: "bg-status-running/20 text-status-running",
    user: "bg-status-review/20 text-status-review",
  };

  function formatValue(val: unknown): string {
    if (val === null || val === undefined) return "—";
    if (typeof val === "boolean") return val ? "Yes" : "No";
    if (typeof val === "string") return val || "—";
    if (typeof val === "number") return String(val);
    if (Array.isArray(val)) return val.map(v => typeof v === "string" ? v : JSON.stringify(v)).join(", ");
    if (typeof val === "object") {
      const entries = Object.entries(val as Record<string, unknown>);
      if (entries.length === 0) return "—";
      return entries.map(([k, v]) => `${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`).join(", ");
    }
    return String(val);
  }

  function shortId(id: string): string {
    return id.slice(0, 8);
  }

  function localTime(ts: string): string {
    const date = new Date(ts);
    if (isNaN(date.getTime())) return ts.slice(11, 19) || "—";
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  }

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
</script>

<div class="space-y-4">
  <!-- ─── Config (collapsed by default) ─────────────────────────────────────── -->
  <section>
    <button
      class="w-full flex items-center gap-2 py-2 hover:bg-panel-hi/30 rounded-md transition-colors -mx-1 px-1"
      onclick={() => configExpanded = !configExpanded}
      aria-expanded={configExpanded}
    >
      {#if configExpanded}<ChevronDown class="size-4 text-t3 shrink-0" />{:else}<ChevronRight class="size-4 text-t3 shrink-0" />{/if}
      <BookOpen class="size-4 text-t2" />
      <span class="text-sm font-semibold text-t2">Config</span>
      <span class="text-xs text-t3 ml-1">{snapshot.recipe_name ?? snapshot.recipe_id}</span>
      {#if snapshot.recipe_source}
        <span class="px-2 py-0.5 rounded-full text-[10px] font-medium {sourceBadgeColors[snapshot.recipe_source] ?? 'bg-t3/20 text-t2'}">
          {snapshot.recipe_source}
        </span>
      {/if}
    </button>

    {#if configExpanded}
      <div class="space-y-4 pt-2 pl-6">
        <!-- Header -->
        <div class="space-y-1">
          {#if snapshot.recipe_path}
            <div class="flex items-center gap-2">
              <code class="text-t3 text-xs font-mono truncate max-w-[300px]">{snapshot.recipe_path}</code>
              {#if onOpenFile}
                <button class="text-accent text-xs hover:underline inline-flex items-center gap-0.5" onclick={() => onOpenFile!(snapshot.recipe_path!)} title="Open recipe file">
                  <ExternalLink class="size-3" /> Open
                </button>
              {/if}
            </div>
          {/if}
          {#if snapshot.recipe_description}
            <p class="text-t3 text-xs">{snapshot.recipe_description}</p>
          {/if}
        </div>

        <!-- Inputs -->
        {#if Object.keys(snapshot.inputs).length > 0}
          <div>
            <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Inputs</h3>
            <div class="border border-border rounded-md overflow-hidden">
              <table class="w-full text-sm">
                <thead class="bg-panel-hi text-t3 text-xs">
                  <tr><th class="text-left px-3 py-1.5">Label</th><th class="text-left px-3 py-1.5">Key</th><th class="text-left px-3 py-1.5">Value</th></tr>
                </thead>
                <tbody>
                  {#each Object.entries(snapshot.inputs) as [key, value] (key)}
                    {@const def = snapshot.input_defs?.[key]}
                    <tr class="border-t border-border">
                      <td class="px-3 py-1.5 text-t2 text-sm">{def?.label ?? key}</td>
                      <td class="px-3 py-1.5 font-mono text-xs text-t3">{key}</td>
                      <td class="px-3 py-1.5 text-t1 text-sm max-w-[300px] truncate" title={formatValue(value)}>{formatValue(value)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/if}

        <!-- Roles -->
        {#if Object.keys(snapshot.roles).length > 0}
          <div>
            <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Roles</h3>
            <div class="border border-border rounded-md overflow-hidden">
              <table class="w-full text-sm">
                <thead class="bg-panel-hi text-t3 text-xs">
                  <tr><th class="text-left px-3 py-1.5">Role</th><th class="text-left px-3 py-1.5">Provider</th><th class="text-left px-3 py-1.5">Mode</th><th class="text-left px-3 py-1.5">Isolation</th></tr>
                </thead>
                <tbody>
                  {#each Object.entries(snapshot.roles) as [name, role] (name)}
                    <tr class="border-t border-border">
                      <td class="px-3 py-1.5 text-t1 font-medium">{name}</td>
                      <td class="px-3 py-1.5 text-t2 font-mono text-xs">{role.provider}</td>
                      <td class="px-3 py-1.5"><span class="px-1.5 py-0.5 rounded text-xs {role.mode === 'write' ? 'bg-status-running/20 text-status-running' : role.mode === 'review' ? 'bg-status-review/20 text-status-review' : 'bg-t3/20 text-t3'}">{role.mode}</span></td>
                      <td class="px-3 py-1.5 text-t3 text-xs">{role.isolation}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/if}

        <!-- Knowledge -->
        {#if snapshot.knowledge.files.length > 0 || snapshot.knowledge.instructions.length > 0}
          <div>
            <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Knowledge</h3>
            {#if snapshot.knowledge.files.length > 0}
              <ul class="space-y-0.5">
                {#each snapshot.knowledge.files as file (file)}
                  <li class="flex items-center gap-2 text-xs">
                    <FileText class="size-3 text-t3" />
                    <code class="text-t2 font-mono">{file}</code>
                    {#if onOpenFile}
                      <button class="text-accent hover:underline text-[10px]" onclick={() => onOpenFile!(file)}>Open</button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if snapshot.knowledge.instructions.length > 0}
              <ul class="space-y-0.5 text-xs text-t2 mt-1">
                {#each snapshot.knowledge.instructions as instruction (instruction)}
                  <li class="flex items-start gap-1.5"><span class="text-t3 shrink-0">•</span><span>{instruction}</span></li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}

        <!-- Tools -->
        {#if snapshot.tools.required.length > 0 || snapshot.tools.optional.length > 0}
          <div>
            <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Tools</h3>
            <div class="flex flex-wrap gap-1.5">
              {#each snapshot.tools.required as tool (tool)}
                <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-status-running/15 text-status-running border border-status-running/30"><Wrench class="size-3" />{tool}</span>
              {/each}
              {#each snapshot.tools.optional as tool (tool)}
                <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-panel-hi text-t3 border border-border"><Wrench class="size-3" />{tool}</span>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Policy -->
        <div>
          <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Policy</h3>
          <div class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
            <span class="text-t3">Max rounds</span><span class="text-t1 font-mono">{snapshot.policy.max_rounds}</span>
            <span class="text-t3">Max ticks</span><span class="text-t1 font-mono">{snapshot.policy.max_ticks}</span>
            <span class="text-t3">Max sessions</span><span class="text-t1 font-mono">{snapshot.policy.max_sessions}</span>
            <span class="text-t3">Merge policy</span><span class="text-t1 font-mono">{snapshot.policy.merge_policy}</span>
            <span class="text-t3">Auto-approve</span><span class="text-t1">{snapshot.policy.auto_approve ? "Yes" : "No"}</span>
          </div>
        </div>
      </div>
    {/if}
  </section>

  <!-- ─── Steps Timeline ──────────────────────────────────────────────────────── -->
  <section>
    <h2 class="text-sm font-semibold text-t2 mb-2">Timeline</h2>
    <ul class="space-y-1">
      {#each snapshot.steps as step (step.id)}
        {@const status = stepStatuses[step.id] ?? "pending"}
        {@const isExpanded = expandedSteps.has(step.id)}
        {@const stepSessions = sessionsByStep[step.id] ?? []}
        {@const stepVerifiers = (step.kind === "gates.run" && step.role) ? (verifierData.byRole[step.role] ?? []) : []}
        {@const stepEvents = eventsByStep.map[step.id] ?? []}
        {@const hasContent = step.prompt || step.on || stepSessions.length > 0 || stepVerifiers.length > 0 || stepEvents.length > 0}
        <li class="rounded-md border border-border overflow-hidden {status === 'active' ? 'border-accent/50 bg-accent/5' : ''}">
          <!-- Step header -->
          <button
            class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hi/50 transition-colors"
            onclick={() => hasContent && toggleStep(step.id)}
            disabled={!hasContent}
            aria-expanded={hasContent ? isExpanded : undefined}
          >
            <span class="shrink-0 text-xs {status === 'done' ? 'text-t2' : status === 'active' ? 'text-accent' : 'text-t3'}">
              {#if status === "done"}●{:else if status === "active"}◉{:else}○{/if}
            </span>
            {#if hasContent}
              {#if isExpanded}<ChevronDown class="size-3 text-t3 shrink-0" />{:else}<ChevronRight class="size-3 text-t3 shrink-0" />{/if}
            {:else}
              <span class="size-3 shrink-0"></span>
            {/if}
            <span class="font-mono text-xs text-t1 min-w-[140px]">{step.id}</span>
            <span class="px-1.5 py-0.5 rounded text-[10px] font-mono bg-panel-hi text-t3">{step.kind}</span>
            {#if step.role}
              <span class="text-xs text-t2">role: {step.role}</span>
            {/if}
            {#if step.from}
              <span class="text-xs text-t3">from: {step.from}</span>
            {/if}
            {#if step.status}
              <span class="text-xs text-t3">→ {step.status}</span>
            {/if}
            {#if stepSessions.length > 0}
              <span class="text-xs text-t3">{stepSessions.length} session{stepSessions.length > 1 ? "s" : ""}</span>
            {/if}
            {#if stepVerifiers.length > 0}
              <span class="text-xs text-t3">{stepVerifiers.length} gate{stepVerifiers.length > 1 ? "s" : ""}</span>
            {/if}
          </button>

          <!-- Expanded details -->
          {#if isExpanded}
            <div class="border-t border-border bg-panel px-3 py-2 space-y-3">
              <!-- Sessions -->
              {#if stepSessions.length > 0}
                <div>
                  <span class="text-[10px] font-semibold text-t3 uppercase">Sessions</span>
                  <div class="mt-1 space-y-1">
                    {#each stepSessions as session (session.session_id)}
                      <div class="flex items-center gap-2 text-xs">
                        <Play class="size-3 text-t3" />
                        <span class="font-mono text-t2">{shortId(session.session_id)}</span>
                        <span class="text-t3">round {session.round}</span>
                        <span class="text-t3">{session.provider ?? ""}</span>
                        <span class="px-1.5 py-0.5 rounded text-[10px] {session.status === 'active' ? 'bg-status-running/20 text-status-running' : 'bg-t3/20 text-t3'}">{session.status}</span>
                        {#if onSelectSession}
                          <button class="text-accent hover:underline text-[10px] ml-auto" onclick={() => onSelectSession!(session.session_id)}>Open</button>
                        {/if}
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}

              <!-- Verifier runs (gates) -->
              {#if stepVerifiers.length > 0}
                <div>
                  <span class="text-[10px] font-semibold text-t3 uppercase">Gates</span>
                  <div class="mt-1 space-y-1">
                    {#each stepVerifiers as vr (vr.id)}
                      {@const VrIcon = verifierIcon(vr.status)}
                      {@const vrOutput = verifierOutputs[vr.id]}
                      <div class="space-y-1">
                        <button
                          class="w-full flex items-center gap-2 text-xs text-left hover:bg-panel-hi/30 rounded px-1 py-0.5"
                          onclick={() => loadVerifierOutput(vr.id, vr.output_path)}
                        >
                          <VrIcon class="size-3 {verifierColor(vr.status)}" />
                          <span class="text-t2 font-medium">{vr.name}</span>
                          <code class="text-t3 font-mono truncate max-w-[200px]">{vr.command}</code>
                          {#if vr.exit_code != null}
                            <span class="ml-auto text-[10px] {vr.exit_code === 0 ? 'text-status-running' : 'text-status-exited'}">exit {vr.exit_code}</span>
                          {/if}
                        </button>
                        {#if vrOutput}
                          <div class="ml-5 rounded bg-panel-hi/50">
                            {#if vrOutput.loading}
                              <div class="px-2 py-1 text-[10px] text-t3">Loading…</div>
                            {:else if vrOutput.error}
                              <div class="px-2 py-1 text-[10px] text-status-exited">{vrOutput.error}</div>
                            {:else if vrOutput.content != null}
                              <pre class="px-2 py-1 text-[10px] font-mono text-t2 overflow-auto max-h-[300px] whitespace-pre-wrap break-words">{vrOutput.content}</pre>
                            {/if}
                          </div>
                        {/if}
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}

              <!-- Routes -->
              {#if step.on}
                <div>
                  <span class="text-[10px] font-semibold text-t3 uppercase">Routes</span>
                  <div class="mt-1 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
                    {#each Object.entries(step.on) as [condition, target] (condition)}
                      <span class="text-t2 font-mono">{condition}</span>
                      <span class="text-accent font-mono">→ {target}</span>
                    {/each}
                  </div>
                </div>
              {/if}

              <!-- Prompt -->
              {#if step.prompt}
                <div>
                  <span class="text-[10px] font-semibold text-t3 uppercase">Prompt</span>
                  <pre class="mt-1 text-xs font-mono text-t2 overflow-auto max-h-[200px] whitespace-pre-wrap break-words bg-panel-hi/50 rounded p-2">{step.prompt}</pre>
                </div>
              {/if}

              <!-- Events for this step -->
              {#if stepEvents.length > 0}
                <div>
                  <span class="text-[10px] font-semibold text-t3 uppercase">Events</span>
                  <ul class="mt-1 space-y-0.5">
                    {#each stepEvents as event (event.id)}
                      <li class="flex items-center gap-2 text-[10px] px-1 py-0.5 rounded hover:bg-panel-hi/30">
                        <span class="text-t2 font-medium">{event.kind}</span>
                        <span class="ml-auto text-t3 font-mono">{localTime(event.ts)}</span>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  </section>

  <!-- ─── Orphan Events (no step_id) ──────────────────────────────────────────── -->
  {#if eventsByStep.orphans.length > 0}
    <section>
      <h2 class="text-sm font-semibold text-t2 mb-2">Loop Events</h2>
      <ul class="space-y-0.5 text-xs">
        {#each eventsByStep.orphans.slice(-20) as event (event.id)}
          <li class="flex items-center gap-2 px-2 py-1 rounded hover:bg-panel-hi/50">
            <span class="text-t2 font-medium min-w-[140px]">{event.kind}</span>
            <span class="ml-auto text-t3 font-mono shrink-0">{localTime(event.ts)}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <!-- ─── Orphan Verifier Runs (not linked to any step) ─────────────────────── -->
  {#if verifierData.orphans.length > 0}
    <section>
      <h2 class="text-sm font-semibold text-t2 mb-2">Unlinked Verifier Runs</h2>
      <ul class="space-y-1">
        {#each verifierData.orphans as vr (vr.id)}
          {@const VrIcon = verifierIcon(vr.status)}
          <li class="flex items-center gap-2 px-3 py-2 rounded-md border border-border">
            <VrIcon class="size-4 {verifierColor(vr.status)}" />
            <span class="text-t1 text-sm font-medium">{vr.name}</span>
            <code class="text-t3 text-xs font-mono truncate max-w-[200px]">{vr.command}</code>
            {#if vr.exit_code != null}
              <span class="ml-auto text-xs {vr.exit_code === 0 ? 'text-status-running' : 'text-status-exited'}">exit {vr.exit_code}</span>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <!-- ─── Orphan Artifacts ────────────────────────────────────────────────────── -->
  {#if artifacts.length > 0}
    <section>
      <h2 class="text-sm font-semibold text-t2 mb-2">Artifacts</h2>
      <ul class="space-y-1">
        {#each artifacts as artifact (artifact.id)}
          <li class="flex items-center gap-2 px-3 py-2 rounded-md border border-border">
            <span class="text-t2 text-sm font-medium">{artifact.kind}</span>
            {#if artifact.path}
              <code class="text-t3 text-xs font-mono flex-1 truncate">{artifact.path}</code>
              {#if onOpenFile}
                <button class="text-accent text-xs hover:underline" onclick={() => onOpenFile!(artifact.path!)}>Open</button>
              {/if}
            {:else}
              <span class="text-t3 text-xs">(inline)</span>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>
