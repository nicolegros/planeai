<script lang="ts">
  import { loops as loopsApi, tasks as tasksApi, projects as projectsApi } from "../lib/api";
  import type { LoopRunSummary, RecipeSummary, TaskItem, RecipeInputDef } from "../lib/types";
  import { Button, Label, Select } from "./ui";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { untrack } from "svelte";
  import { LoaderCircle } from "@lucide/svelte";

  interface Props {
    projects: { id: string; name: string; path: string }[];
    onCreated: (loop: LoopRunSummary) => void;
    onCancel: () => void;
    taskKey?: string | null;
  }

  let { projects, onCreated, onCancel, taskKey = null }: Props = $props();

  // svelte-ignore state_referenced_locally
  let selectedProjectId = $state(projects[0]?.id ?? "");
  const selectedProject = $derived(projects.find(p => p.id === selectedProjectId));

  let recipeId = $state("");
  let maxRounds = $state(3);
  let draft = $state(false);
  let submitting = $state(false);

  let recipes = $state<RecipeSummary[]>([]);
  let taskItems = $state<TaskItem[]>([]);
  let branches = $state<{ value: string; label: string }[]>([]);

  // Dynamic input values keyed by input name
  let inputValues = $state<Record<string, unknown>>({});

  const selectedRecipe = $derived(recipes.find(r => r.id === recipeId));
  const recipeInputs = $derived(selectedRecipe?.inputs ?? {});
  const recipeInputEntries = $derived(Object.entries(recipeInputs));

  // Track previous recipe to detect changes
  let lastRecipeId = "";

  // Initialize input values when recipe changes
  $effect(() => {
    const currentRecipeId = recipeId;
    const entries = recipeInputEntries;
    // Only reset values when recipe actually changes
    if (currentRecipeId === lastRecipeId) return;
    lastRecipeId = currentRecipeId;

    untrack(() => {
      const newValues: Record<string, unknown> = {};
      for (const [key, def] of entries) {
        if (def.default !== undefined && def.default !== null) {
          newValues[key] = def.default;
        } else if (def.input_type === "boolean") {
          newValues[key] = false;
        } else {
          newValues[key] = "";
        }
      }
      // If taskKey prop was passed and there's a task input, pre-fill it
      if (taskKey) {
        for (const [key, def] of entries) {
          if (def.input_type === "task") {
            newValues[key] = taskKey;
            break;
          }
        }
      }
      inputValues = newValues;
    });
  });



  // Load recipes when project changes
  $effect(() => {
    if (!selectedProjectId) return;
    loopsApi.recipes(selectedProjectId).then(
      (r) => {
        recipes = r;
        if (r.length > 0 && !recipeId) recipeId = r[0].id;
      },
      () => (recipes = []),
    );
  });

  // Load tasks when project changes (needed for task-type inputs)
  $effect(() => {
    const path = selectedProject?.path;
    if (!path) return;
    tasksApi.list(path).then(
      (items) => (taskItems = items),
      () => (taskItems = []),
    );
  });

  // Load branches when project changes (needed for branch-type inputs)
  $effect(() => {
    const path = selectedProject?.path;
    if (!path) return;
    projectsApi.listBranches(path).then(
      (b) => {
        branches = b
          .filter((s) => !s.startsWith("remote:"))
          .map((s) => ({ value: s, label: s }));
      },
      () => (branches = []),
    );
  });

  const recipeItems = $derived(
    recipes.map((r) => ({
      value: r.id,
      label: r.description ? `${r.name} — ${r.description}` : r.name,
    })),
  );

  const taskSelectItems = $derived([
    { value: "", label: "None" },
    ...taskItems.map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` })),
  ]);

  // Track whether user has attempted to submit (to show errors only after first attempt)
  let submitAttempted = $state(false);

  // Per-field validation errors
  const inputErrors = $derived.by(() => {
    const errors: Record<string, string> = {};
    for (const [key, def] of recipeInputEntries) {
      if (!def.required) continue;
      const val = inputValues[key];
      if (val === undefined || val === null || val === "") {
        errors[key] = "Required";
      }
    }
    return errors;
  });

  // Validation: all required inputs must have non-empty values
  const canSubmit = $derived.by(() => {
    if (!recipeId || submitting) return false;
    for (const [key, def] of recipeInputEntries) {
      if (!def.required) continue;
      const val = inputValues[key];
      if (val === undefined || val === null || val === "") return false;
    }
    return true;
  });

  async function submit() {
    submitAttempted = true;
    if (!canSubmit) return;
    submitting = true;

    try {
      // Collect inputs, trimming strings
      const inputs: Record<string, unknown> = {};
      for (const [key, def] of recipeInputEntries) {
        const val = inputValues[key];
        const type = def.input_type;
        if (type === "boolean") {
          inputs[key] = val;
        } else if (typeof val === "string") {
          const trimmed = val.trim();
          if (trimmed) inputs[key] = trimmed;
        } else if (val !== undefined && val !== null && val !== "") {
          inputs[key] = val;
        }
      }

      const result = await loopsApi.create({
        projectId: selectedProjectId,
        recipeId,
        inputs,
        maxRounds,
        start: !draft,
      });
      onCreated(result);
    } catch (err) {
      showSnackbar(`Failed to create loop: ${err}`);
      submitting = false;
    }
  }

  // Shortcut keys for dynamic inputs: assign first letter of each key (deduplicated)
  const RESERVED_KEYS = new Set(["p", "r", "m", "d"]);
  const inputShortcuts = $derived.by(() => {
    const map: Record<string, string> = {};
    const used = new Set(RESERVED_KEYS);
    for (const [key] of recipeInputEntries) {
      // Try first letter, then subsequent letters
      let assigned = "";
      for (const ch of key.toLowerCase().replace(/[^a-z]/g, "")) {
        if (!used.has(ch)) {
          assigned = ch;
          used.add(ch);
          break;
        }
      }
      map[key] = assigned;
    }
    return map;
  });

  // Form keyboard controller
  let wrapperEl: HTMLDivElement | undefined = $state();

  const fk = createFormKeyboardController(
    () => [
      ...(projects.length > 1 ? [{ key: "p", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='project'] input") ?? null }] : []),
      { key: "r", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='recipe'] input") ?? null },
      // Dynamic input field bindings
      ...recipeInputEntries.map(([key]) => {
        const shortcut = inputShortcuts[key];
        if (!shortcut) return null;
        return {
          key: shortcut,
          ref: () => wrapperEl?.querySelector<HTMLElement>(`[data-field='input-${key}'] input, [data-field='input-${key}'] textarea`) ?? null,
        };
      }).filter((b): b is NonNullable<typeof b> => b !== null),
      { key: "m", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='max-rounds'] input") ?? null },
      { key: "d", toggle: () => (draft = !draft) },
    ],
    { wrapper: () => wrapperEl ?? null, onDismiss: () => onCancel() },
  );

  const badge = $derived(fk.mode === "normal" ? "bg-accent-bg text-accent" : "bg-panel-hi text-t3");

  function metaEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); }
  }

  function getInputLabel(key: string, def: RecipeInputDef): string {
    return def.label ?? key.replace(/_/g, " ").replace(/\b\w/g, c => c.toUpperCase());
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div bind:this={wrapperEl} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none" data-form-keyboard>
<form class="px-5 pb-0 space-y-3" onsubmit={(e) => { e.preventDefault(); submit(); }}>

  {#if projects.length > 1}
    <div class="space-y-1" data-field="project">
      <Label>Project <span class="font-mono text-[10px] px-1 rounded {badge}">P</span></Label>
      <Select
        items={projects.map(p => ({ value: p.id, label: p.name }))}
        bind:value={selectedProjectId}
        onkeydown={metaEnter}
        placeholder="Select project…"
      />
    </div>
  {/if}

  <div class="space-y-1" data-field="recipe">
    <Label>Recipe <span class="font-mono text-[10px] px-1 rounded {badge}">R</span></Label>
    {#if recipeItems.length > 0}
      <Select
        items={recipeItems}
        bind:value={recipeId}
        onkeydown={metaEnter}
        placeholder="Select recipe…"
      />
    {:else}
      <p class="text-t3 text-sm">Loading recipes…</p>
    {/if}
  </div>

  {#each recipeInputEntries as [key, def] (key)}
    {@const inputType = def.input_type}
    {@const label = getInputLabel(key, def)}
    {@const shortcut = inputShortcuts[key]}
    {@const isRequired = def.required}

    <div class="space-y-1" data-field="input-{key}">
      {#if inputType === "boolean"}
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="input-{key}"
            checked={!!inputValues[key]}
            onchange={(e) => { inputValues[key] = (e.currentTarget as HTMLInputElement).checked; }}
            class="rounded border-border accent-accent focus:ring-accent"
            data-input-key={key}
          />
          <Label for="input-{key}">
            {label}
            {#if isRequired}<span class="text-red-400">*</span>{/if}
            {#if shortcut}<span class="font-mono text-[10px] px-1 rounded {badge}">{shortcut.toUpperCase()}</span>{/if}
          </Label>
        </div>
      {:else}
        <Label>
          {label}
          {#if isRequired}<span class="text-red-400">*</span>{/if}
          {#if shortcut}<span class="font-mono text-[10px] px-1 rounded {badge}">{shortcut.toUpperCase()}</span>{/if}
        </Label>

        {#if inputType === "textarea"}
          <textarea
            value={String(inputValues[key] ?? "")}
            oninput={(e) => { inputValues[key] = (e.currentTarget as HTMLTextAreaElement).value; }}
            onkeydown={metaEnter}
            data-input-key={key}
            class="w-full rounded-md border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent resize-y min-h-[60px]"
            placeholder={def.description ?? ""}
            rows="3"
          ></textarea>
        {:else if inputType === "branch"}
          <Select
            items={branches}
            value={String(inputValues[key] ?? "")}
            onValueChange={(v) => { inputValues[key] = v; }}
            onkeydown={metaEnter}
            placeholder="Select branch…"
            emptyText="No branches found"
          />
        {:else if inputType === "task"}
          <Select
            items={taskSelectItems}
            value={String(inputValues[key] ?? "")}
            onValueChange={(v) => { inputValues[key] = v; }}
            onkeydown={metaEnter}
            placeholder="Link to task…"
          />
        {:else if inputType === "select"}
          <Select
            items={def.options ?? []}
            value={String(inputValues[key] ?? "")}
            onValueChange={(v) => { inputValues[key] = v; }}
            onkeydown={metaEnter}
            placeholder="Select…"
          />
        {:else if inputType === "number"}
          <input
            type="number"
            value={inputValues[key] != null ? Number(inputValues[key]) : ""}
            oninput={(e) => { inputValues[key] = (e.currentTarget as HTMLInputElement).valueAsNumber; }}
            onkeydown={metaEnter}
            data-input-key={key}
            class="w-full rounded-md border border-border bg-panel px-3 py-1.5 text-sm text-t1 focus:outline-none focus:ring-1 focus:ring-accent"
          />
        {:else}
          <!-- text or unknown type: single-line input -->
          <input
            type="text"
            value={String(inputValues[key] ?? "")}
            oninput={(e) => { inputValues[key] = (e.currentTarget as HTMLInputElement).value; }}
            onkeydown={metaEnter}
            data-input-key={key}
            placeholder={def.description ?? ""}
            class="w-full rounded-md border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent"
          />
        {/if}
      {/if}

      {#if def.description && inputType !== "textarea" && inputType !== "text"}
        <p class="text-xs text-t3">{def.description}</p>
      {/if}

      {#if submitAttempted && inputErrors[key]}
        <p class="text-xs text-red-400 mt-0.5">{inputErrors[key]}</p>
      {/if}
    </div>
  {/each}

  <div class="space-y-1" data-field="max-rounds">
    <Label>Max rounds <span class="font-mono text-[10px] px-1 rounded {badge}">M</span></Label>
    <input
      type="number"
      bind:value={maxRounds}
      onkeydown={metaEnter}
      min="1"
      max="20"
      class="w-20 rounded-md border border-border bg-panel px-3 py-1.5 text-sm text-t1 focus:outline-none focus:ring-1 focus:ring-accent"
    />
  </div>

  <div class="flex items-center gap-2" data-field="draft">
    <input
      type="checkbox"
      id="loop-draft"
      data-field="draft"
      bind:checked={draft}
      class="rounded border-border"
    />
    <Label for="loop-draft">Start as draft <span class="font-mono text-[10px] px-1 rounded {badge}">D</span></Label>
  </div>

  <div class="flex items-center justify-between pt-2 pb-4 border-t border-border mt-3">
    <div class="flex items-center gap-2">
      {#if fk.mode === "insert"}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-accent-bg text-accent font-medium">INSERT</span>
        <span class="text-[10px] text-t3">esc → normal mode</span>
      {:else}
        <span class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-panel-hi text-t2 font-medium">NORMAL</span>
        <span class="text-[10px] text-t3">press a key to focus field</span>
      {/if}
    </div>
    <div class="flex gap-2">
      <Button type="button" onclick={() => onCancel()}>Cancel</Button>
      <Button type="submit" variant="primary" disabled={!canSubmit}>
        {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}Start loop <span class="ml-1 font-mono text-[10px] opacity-60">{MOD_ENTER_HINT}</span>{/if}
      </Button>
    </div>
  </div>
</form>
</div>
