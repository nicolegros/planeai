<script lang="ts">
  import { loops as loopsApi, tasks as tasksApi } from "../lib/api";
  import type { LoopRunSummary, RecipeSummary, TaskItem } from "../lib/types";
  import { Button, Label, Select, Checkbox } from "./ui";
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

  async function handleSubmit(e: Event) {
    e.preventDefault();
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
  let formRef: HTMLDivElement | undefined = $state();
  let goalRef: HTMLTextAreaElement | undefined = $state();

  const formKb = createFormKeyboardController(
    () => [
      { key: "g", ref: () => goalRef ?? null },
      { key: "d", toggle: () => (draft = !draft) },
    ],
    { wrapper: () => formRef ?? null, onDismiss: () => onCancel() },
  );

  function handleFormKeydown(e: KeyboardEvent) {
    // Mod+Enter submits
    if (e.key === "Enter" && isPlatformMod(e)) {
      e.preventDefault();
      if (canSubmit) handleSubmit(e);
      return;
    }
    formKb.handleKeydown(e);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  bind:this={formRef}
  class="w-[500px] p-5 space-y-4"
  tabindex="0"
  role="dialog"
  aria-label="Create Loop"
  onkeydown={handleFormKeydown}
  onfocusin={formKb.handleFocusin}
>
  <h2 class="text-lg font-semibold text-t1">Start Loop</h2>

  <form onsubmit={handleSubmit} class="space-y-4">
    <!-- Goal -->
    <div>
      <Label for="loop-goal">Goal <kbd class="text-t3 text-xs ml-1">g</kbd></Label>
      <textarea
        bind:this={goalRef}
        bind:value={goal}
        data-field="goal"
        id="loop-goal"
        class="mt-1 w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent resize-y min-h-[60px]"
        placeholder="What should this loop accomplish?"
        rows="3"
      ></textarea>
    </div>

    <!-- Recipe -->
    <div>
      <Label for="loop-recipe">Recipe</Label>
      {#if recipeItems.length > 0}
        <Select
          items={recipeItems}
          bind:value={recipeId}
          placeholder="Select recipe…"
        />
      {:else}
        <p class="text-t3 text-sm mt-1">Loading recipes…</p>
      {/if}
    </div>

    <!-- Task -->
    <div>
      <Label for="loop-task">Task (optional)</Label>
      <Select
        items={taskSelectItems}
        bind:value={selectedTaskKey}
        placeholder="Link to task…"
      />
    </div>

    <!-- Max Rounds -->
    <div>
      <Label for="loop-max-rounds">Max rounds</Label>
      <input
        type="number"
        id="loop-max-rounds"
        data-field="max-rounds"
        bind:value={maxRounds}
        min="1"
        max="20"
        class="mt-1 w-20 rounded border border-border bg-panel px-3 py-1.5 text-sm text-t1 focus:outline-none focus:ring-1 focus:ring-accent"
      />
    </div>

    <!-- Draft checkbox -->
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="loop-draft"
        data-field="draft"
        bind:checked={draft}
        class="rounded border-border"
      />
      <Label for="loop-draft">Start as draft (don't run immediately)</Label>
    </div>

    <!-- Actions -->
    <div class="flex items-center justify-between pt-2">
      <span class="text-t3 text-xs">{MOD_ENTER_HINT} to submit</span>
      <div class="flex gap-2">
        <Button variant="ghost" onclick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!canSubmit}>
          {#if submitting}
            <LoaderCircle class="w-4 h-4 animate-spin" />
          {:else}
            Start Loop
          {/if}
        </Button>
      </div>
    </div>
  </form>
</div>
