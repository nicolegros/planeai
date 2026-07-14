<script lang="ts">
  import type { RecipeSnapshot, RecipeInputDef, RecipeStepDef } from "../lib/types";
  import { ChevronRight, ChevronDown, ExternalLink, FileText, Wrench, BookOpen, Shield } from "@lucide/svelte";

  // ─── Props ─────────────────────────────────────────────────────────────────────

  interface Props {
    snapshot: RecipeSnapshot;
    onOpenFile?: (path: string) => void;
  }

  let { snapshot, onOpenFile }: Props = $props();

  // ─── State ─────────────────────────────────────────────────────────────────────

  let expanded = $state(false);
  let expandedSteps = $state<Set<string>>(new Set());

  function toggleStep(stepId: string) {
    if (expandedSteps.has(stepId)) {
      expandedSteps = new Set([...expandedSteps].filter(id => id !== stepId));
    } else {
      expandedSteps = new Set([...expandedSteps, stepId]);
    }
  }

  // ─── Derived ───────────────────────────────────────────────────────────────────

  // Determine step status: steps before current_step are "done", current is "active", rest "pending"
  const stepStatuses = $derived.by(() => {
    const statuses: Record<string, "done" | "active" | "pending"> = {};
    const currentIdx = snapshot.steps.findIndex(s => s.id === snapshot.runtime.current_step);
    for (let i = 0; i < snapshot.steps.length; i++) {
      if (i < currentIdx) statuses[snapshot.steps[i].id] = "done";
      else if (i === currentIdx) statuses[snapshot.steps[i].id] = "active";
      else statuses[snapshot.steps[i].id] = "pending";
    }
    return statuses;
  });

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
      // Render object as "key: value" pairs
      const entries = Object.entries(val as Record<string, unknown>);
      if (entries.length === 0) return "—";
      return entries.map(([k, v]) => `${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`).join(", ");
    }
    return String(val);
  }
</script>

<section>
  <!-- Collapsible trigger -->
  <button
    class="w-full flex items-center gap-2 py-2 hover:bg-panel-hi/30 rounded-md transition-colors -mx-1 px-1"
    onclick={() => expanded = !expanded}
  >
    {#if expanded}<ChevronDown class="size-4 text-t3 shrink-0" />{:else}<ChevronRight class="size-4 text-t3 shrink-0" />{/if}
    <BookOpen class="size-4 text-t2" />
    <h2 class="text-sm font-semibold text-t2">Recipe Definition</h2>
    <span class="text-xs text-t3 ml-1">{snapshot.recipe_name ?? snapshot.recipe_id}</span>
  </button>

  {#if expanded}
    <div class="space-y-5 pt-2 pl-6">
      <!-- ─── Header ──────────────────────────────────────────────────────────── -->
      <div class="space-y-1">
        <div class="flex items-center gap-2 flex-wrap">
          <span class="text-t1 font-medium text-sm">{snapshot.recipe_name ?? snapshot.recipe_id}</span>
          <span class="px-2 py-0.5 rounded-full text-xs font-medium {sourceBadgeColors[snapshot.recipe_source] ?? 'bg-t3/20 text-t2'}">
            {snapshot.recipe_source}
          </span>
          {#if snapshot.recipe_path}
            <code class="text-t3 text-xs font-mono truncate max-w-[300px]">{snapshot.recipe_path}</code>
            {#if onOpenFile}
              <button
                class="text-accent text-xs hover:underline inline-flex items-center gap-0.5"
                onclick={() => onOpenFile!(snapshot.recipe_path!)}
                title="Open recipe file"
              >
                <ExternalLink class="size-3" />
                Open
              </button>
            {/if}
          {/if}
        </div>
        {#if snapshot.recipe_description}
          <p class="text-t3 text-xs">{snapshot.recipe_description}</p>
        {/if}
      </div>

      <!-- ─── Inputs ──────────────────────────────────────────────────────────── -->
      {#if Object.keys(snapshot.inputs).length > 0}
        <div>
          <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Inputs</h3>
          <div class="border border-border rounded-md overflow-hidden">
            <table class="w-full text-sm">
              <thead class="bg-panel-hi text-t3 text-xs">
                <tr>
                  <th class="text-left px-3 py-1.5">Label</th>
                  <th class="text-left px-3 py-1.5">Key</th>
                  <th class="text-left px-3 py-1.5">Value</th>
                </tr>
              </thead>
              <tbody>
                {#each Object.entries(snapshot.inputs) as [key, value] (key)}
                  {@const def = snapshot.input_defs?.[key]}
                  <tr class="border-t border-border">
                    <td class="px-3 py-1.5 text-t2 text-sm">{def?.label ?? key}</td>
                    <td class="px-3 py-1.5 font-mono text-xs text-t3">{key}</td>
                    <td class="px-3 py-1.5 text-t1 text-sm max-w-[300px] truncate" title={formatValue(value)}>
                      {formatValue(value)}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {/if}

      <!-- ─── Roles ───────────────────────────────────────────────────────────── -->
      {#if Object.keys(snapshot.roles).length > 0}
        <div>
          <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Roles</h3>
          <div class="border border-border rounded-md overflow-hidden">
            <table class="w-full text-sm">
              <thead class="bg-panel-hi text-t3 text-xs">
                <tr>
                  <th class="text-left px-3 py-1.5">Role</th>
                  <th class="text-left px-3 py-1.5">Provider</th>
                  <th class="text-left px-3 py-1.5">Mode</th>
                  <th class="text-left px-3 py-1.5">Isolation</th>
                </tr>
              </thead>
              <tbody>
                {#each Object.entries(snapshot.roles) as [name, role] (name)}
                  <tr class="border-t border-border">
                    <td class="px-3 py-1.5 text-t1 font-medium">{name}</td>
                    <td class="px-3 py-1.5 text-t2 font-mono text-xs">{role.provider}</td>
                    <td class="px-3 py-1.5">
                      <span class="px-1.5 py-0.5 rounded text-xs {role.mode === 'write' ? 'bg-status-running/20 text-status-running' : role.mode === 'review' ? 'bg-status-review/20 text-status-review' : 'bg-t3/20 text-t3'}">
                        {role.mode}
                      </span>
                    </td>
                    <td class="px-3 py-1.5 text-t3 text-xs">{role.isolation}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {/if}

      <!-- ─── Steps ───────────────────────────────────────────────────────────── -->
      {#if snapshot.steps.length > 0}
        <div>
          <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Steps</h3>
          <ul class="space-y-0.5">
            {#each snapshot.steps as step (step.id)}
              {@const status = stepStatuses[step.id] ?? "pending"}
              {@const isExpanded = expandedSteps.has(step.id)}
              {@const hasDetails = step.prompt || step.on}
              <li class="rounded-md border border-border overflow-hidden {status === 'active' ? 'border-accent/50 bg-accent/5' : ''}">
                <button
                  class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-panel-hi/50 transition-colors"
                  onclick={() => hasDetails && toggleStep(step.id)}
                  disabled={!hasDetails}
                >
                  <!-- Status indicator -->
                  <span class="shrink-0 text-xs {status === 'done' ? 'text-t2' : status === 'active' ? 'text-accent' : 'text-t3'}">
                    {#if status === "done"}●{:else if status === "active"}◉{:else}○{/if}
                  </span>

                  <!-- Expand chevron (only if has details) -->
                  {#if hasDetails}
                    {#if isExpanded}<ChevronDown class="size-3 text-t3 shrink-0" />{:else}<ChevronRight class="size-3 text-t3 shrink-0" />{/if}
                  {:else}
                    <span class="size-3 shrink-0"></span>
                  {/if}

                  <!-- Step info -->
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
                  {#if step.gates && step.gates.length > 0}
                    <span class="text-xs text-t3">{step.gates.length} gate{step.gates.length > 1 ? "s" : ""}</span>
                  {/if}
                </button>

                <!-- Expanded details -->
                {#if isExpanded}
                  <div class="border-t border-border bg-panel px-3 py-2 space-y-2">
                    <!-- On routing sub-table -->
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

                    <!-- Gates sub-table -->
                    {#if step.gates && step.gates.length > 0}
                      <div>
                        <span class="text-[10px] font-semibold text-t3 uppercase">Gates</span>
                        <div class="mt-1 space-y-0.5">
                          {#each step.gates as gate (gate.name)}
                            <div class="flex items-center gap-2 text-xs">
                              <Shield class="size-3 text-t3" />
                              <span class="text-t2 font-medium">{gate.name}</span>
                              <code class="text-t3 font-mono">{gate.command}</code>
                            </div>
                          {/each}
                        </div>
                      </div>
                    {/if}

                    <!-- Prompt (expandable) -->
                    {#if step.prompt}
                      <div>
                        <span class="text-[10px] font-semibold text-t3 uppercase">Prompt</span>
                        <pre class="mt-1 text-xs font-mono text-t2 overflow-auto max-h-[200px] whitespace-pre-wrap break-words bg-panel-hi/50 rounded p-2">{step.prompt}</pre>
                      </div>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <!-- ─── Knowledge ───────────────────────────────────────────────────────── -->
      {#if snapshot.knowledge.files.length > 0 || snapshot.knowledge.instructions.length > 0}
        <div>
          <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Knowledge</h3>
          <div class="space-y-2">
            {#if snapshot.knowledge.files.length > 0}
              <ul class="space-y-0.5">
                {#each snapshot.knowledge.files as file (file)}
                  <li class="flex items-center gap-2 text-xs">
                    <FileText class="size-3 text-t3" />
                    <code class="text-t2 font-mono">{file}</code>
                    {#if onOpenFile}
                      <button
                        class="text-accent hover:underline text-[10px]"
                        onclick={() => onOpenFile!(file)}
                      >
                        Open
                      </button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if snapshot.knowledge.instructions.length > 0}
              <ul class="space-y-0.5 text-xs text-t2">
                {#each snapshot.knowledge.instructions as instruction (instruction)}
                  <li class="flex items-start gap-1.5">
                    <span class="text-t3 shrink-0">•</span>
                    <span>{instruction}</span>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        </div>
      {/if}

      <!-- ─── Tools ───────────────────────────────────────────────────────────── -->
      {#if snapshot.tools.required.length > 0 || snapshot.tools.optional.length > 0}
        <div>
          <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Tools</h3>
          <div class="flex flex-wrap gap-1.5">
            {#each snapshot.tools.required as tool (tool)}
              <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-status-running/15 text-status-running border border-status-running/30">
                <Wrench class="size-3" />
                {tool}
              </span>
            {/each}
            {#each snapshot.tools.optional as tool (tool)}
              <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-panel-hi text-t3 border border-border">
                <Wrench class="size-3" />
                {tool}
              </span>
            {/each}
          </div>
          {#if snapshot.tools.optional.length > 0}
            <p class="text-[10px] text-t3 mt-1">Outlined = optional</p>
          {/if}
        </div>
      {/if}

      <!-- ─── Policy ──────────────────────────────────────────────────────────── -->
      <div>
        <h3 class="text-xs font-semibold text-t3 uppercase tracking-wide mb-1.5">Policy</h3>
        <div class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
          <span class="text-t3">Max rounds</span>
          <span class="text-t1 font-mono">{snapshot.policy.max_rounds}</span>
          <span class="text-t3">Max ticks</span>
          <span class="text-t1 font-mono">{snapshot.policy.max_ticks}</span>
          <span class="text-t3">Max sessions</span>
          <span class="text-t1 font-mono">{snapshot.policy.max_sessions}</span>
          <span class="text-t3">Merge policy</span>
          <span class="text-t1 font-mono">{snapshot.policy.merge_policy}</span>
          <span class="text-t3">Auto-approve</span>
          <span class="text-t1">{snapshot.policy.auto_approve ? "Yes" : "No"}</span>
        </div>
      </div>
    </div>
  {/if}
</section>
