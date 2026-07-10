<script lang="ts">
  import { loops as loopsApi, tasks as tasksApi } from "../lib/api";
  import type { LoopRunSummary, RecipeSummary, TaskItem } from "../lib/types";
  import { Button, Input, Label, Select } from "./ui";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { LoaderCircle } from "@lucide/svelte";

  interface Props {
    projectId: string;
    projectPath: string;
    onCreated: (loop: LoopRunSummary) => void;
    onCancel: () => void;
    taskKey?: string | null;
  }

  let { projectId, projectPath, onCreated, onCancel, taskKey = null }: Props = $props();

  let goal = $state("");
  let recipeId = $state("");
  // svelte-ignore state_referenced_locally
  let selectedTaskKey = $state(taskKey ?? "");
  let maxRounds = $state(3);
  let draft = $state(false);
  let submitting = $state(false);

  let recipes = $state<RecipeSummary[]>([]);
  let taskItems = $state<TaskItem[]>([]);

  // Load recipes on mount
  $effect(() => {
    loopsApi.recipes(projectId).then(
      (r) => {
        recipes = r;
        if (r.length > 0 && !recipeId) recipeId = r[0].id;
      },
      () => (recipes = []),
    );
  });

  // Load tasks for task key selector
  $effect(() => {
    tasksApi.list(projectPath).then(
      (items) => (taskItems = items),
      () => (taskItems = []),
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

  const canSubmit = $derived(goal.trim().length > 0 && recipeId.length > 0 && !submitting);

  async function submit() {
    if (!canSubmit) return;
    submitting = true;

    try {
      const result = await loopsApi.create({
        projectId,
        goal: goal.trim(),
        recipeId,
        taskKey: selectedTaskKey || null,
        maxRounds,
        start: !draft,
      });
      onCreated(result);
    } catch (err) {
      showSnackbar(`Failed to create loop: ${err}`);
      submitting = false;
    }
  }

  // Form keyboard controller
  let wrapperEl: HTMLDivElement | undefined = $state();
  let goalRef: HTMLTextAreaElement | undefined = $state();

  const badge = $derived(fk.mode === "normal" ? "bg-accent-bg text-accent" : "bg-panel-hi text-t3");

  const fk = createFormKeyboardController(
    () => [
      { key: "g", ref: () => goalRef ?? null },
      { key: "r", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='recipe'] input") ?? null },
      { key: "t", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='task'] input") ?? null },
      { key: "m", ref: () => wrapperEl?.querySelector<HTMLElement>("[data-field='max-rounds'] input") ?? null },
      { key: "d", toggle: () => (draft = !draft) },
    ],
    { wrapper: () => wrapperEl ?? null, onDismiss: () => onCancel() },
  );

  function metaEnter(e: KeyboardEvent) {
    if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div bind:this={wrapperEl} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none" data-form-keyboard>
<form class="px-5 pb-0 space-y-3" onsubmit={(e) => { e.preventDefault(); submit(); }}>

  <div class="space-y-1" data-field="goal">
    <Label>Goal <span class="font-mono text-[10px] px-1 rounded {badge}">G</span></Label>
    <textarea
      bind:this={goalRef}
      bind:value={goal}
      data-field="goal"
      onkeydown={metaEnter}
      class="w-full rounded-md border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent resize-y min-h-[60px]"
      placeholder="What should this loop accomplish?"
      rows="3"
    ></textarea>
  </div>

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

  <div class="space-y-1" data-field="task">
    <Label>Task <span class="font-mono text-[10px] px-1 rounded {badge}">T</span></Label>
    <Select
      items={taskSelectItems}
      bind:value={selectedTaskKey}
      onkeydown={metaEnter}
      placeholder="Link to task…"
    />
  </div>

  <div class="space-y-1" data-field="max-rounds">
    <Label>Max rounds <span class="font-mono text-[10px] px-1 rounded {badge}">M</span></Label>
    <Input
      type="number"
      bind:value={maxRounds}
      onkeydown={metaEnter}
      min="1"
      max="20"
      class="w-20"
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
