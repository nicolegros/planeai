<script lang="ts">
  import type { Project, TaskItem } from "../lib/types";
  import { projects as projectsApi } from "../lib/api";
  import { Button, Input, Label, Select, PillInput, PillCombobox } from "./ui";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { LoaderCircle } from "@lucide/svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    mode: "create" | "edit";
    projects: Project[];
    tasks?: TaskItem[];
    /** Pre-fill values for edit mode */
    initial?: {
      key?: string;
      title?: string;
      description?: string;
      priority?: number;
      parentKey?: string | null;
      blockedBy?: string[];
      tags?: string[];
      baseBranch?: string;
      projectPath?: string;
    };
    onSubmitted: () => void;
    onCancel: () => void;
  }

  let { mode, projects, tasks = [], initial = {}, onSubmitted, onCancel }: Props = $props();

  // svelte-ignore state_referenced_locally
  let formTitle = $state(initial.title ?? "");
  // svelte-ignore state_referenced_locally
  let formDescription = $state(initial.description ?? "");
  // svelte-ignore state_referenced_locally
  let formPriority = $state(initial.priority ?? 0);
  // svelte-ignore state_referenced_locally
  let formParentKey = $state(initial.parentKey ?? "");
  // svelte-ignore state_referenced_locally
  let formBlockedBy = $state<string[]>(initial.blockedBy ?? []);
  // svelte-ignore state_referenced_locally
  let formTags = $state<string[]>(initial.tags ?? []);
  // svelte-ignore state_referenced_locally
  let formBaseBranch = $state(initial.baseBranch ?? "main");
  // svelte-ignore state_referenced_locally
  let formProjectPath = $state(initial.projectPath ?? projects[0]?.path ?? "");
  let formWrapper = $state<HTMLDivElement | null>(null);

  let branches = $state<{ value: string; label: string }[]>([]);

  // Fetch branches eagerly on open and when project changes
  $effect(() => {
    if (formProjectPath) {
      projectsApi.listBranches(formProjectPath).then(
        (b) => {
          const seen = new Set<string>();
          branches = b
            .map((s) => {
              const remote = s.startsWith("remote:");
              const name = remote ? s.slice(7) : s;
              return { value: name, label: name };
            })
            .filter((item) => {
              if (seen.has(item.value)) return false;
              seen.add(item.value);
              return true;
            });
        },
        () => (branches = []),
      );
    }
  });

  // Derive task items for parent_key and blocked_by comboboxes — scoped to current project
  const projectTasks = $derived.by(() => {
    // Prefer tasks for the selected project path; fall back to all tasks passed in
    const fromStore = taskStore.getTasksForProject(formProjectPath);
    const pool = fromStore.length > 0 ? fromStore : tasks;
    // Deduplicate by key (safety net against flat() duplicates)
    const seen = new Set<string>();
    return pool.filter((t) => {
      if (seen.has(t.key)) return false;
      seen.add(t.key);
      return true;
    });
  });

  const parentItems = $derived(
    projectTasks
      .filter((t) => t.key !== initial.key) // Can't be own parent
      .map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` }))
  );

  const blockerItems = $derived(
    projectTasks
      .filter((t) => t.key !== initial.key) // Can't block self
      .map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` }))
  );

  const fk = createFormKeyboardController(
    () => [
      { key: "t", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='title'] input") ?? null },
      { key: "d", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='desc'] textarea") ?? null },
      { key: "p", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='priority'] input") ?? null },
      { key: "r", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='parent'] input") ?? null },
      { key: "k", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='blocked'] input") ?? null },
      { key: "g", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='tags'] input") ?? null },
      { key: "b", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='base'] input") ?? null },
    ],
    { wrapper: () => formWrapper, onDismiss: () => onCancel() },
  );

  const badge = $derived(fk.mode === "normal" ? "bg-accent-bg text-accent" : "bg-panel-hi text-t3");

  let submitting = $state(false);

  async function handleSubmit() {
    if (!formTitle.trim() || submitting) return;
    submitting = true;
    try {
      if (mode === "create") {
        const repoPath = formProjectPath || projects[0]?.path;
        if (!repoPath) return;
        await taskStore.createTask({
          repoPath,
          title: formTitle.trim(),
          description: formDescription,
          priority: formPriority,
          tags: formTags,
          blockedBy: formBlockedBy,
          parentKey: formParentKey || null,
          baseBranch: formBaseBranch,
        });
      } else {
        const repoPath = formProjectPath || projects[0]?.path;
        if (!repoPath || !initial.key) return;
        await taskStore.editTask({
          repoPath,
          key: initial.key,
          title: formTitle.trim(),
          description: formDescription,
          priority: formPriority,
          tags: formTags,
          blockedBy: formBlockedBy,
          parentKey: formParentKey || null,
          baseBranch: formBaseBranch || null,
        });
      }
      onSubmitted();
    } catch (e: any) {
      showSnackbar(e.toString());
      submitting = false;
    }
  }

  function autofocusForm(node: HTMLFormElement) {
    requestAnimationFrame(() => node.querySelector<HTMLInputElement>("input")?.focus());
  }

  function autoResize(node: HTMLTextAreaElement) {
    requestAnimationFrame(() => { node.style.height = "auto"; node.style.height = node.scrollHeight + "px"; });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div bind:this={formWrapper} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); handleSubmit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none px-5 pb-5" data-form-keyboard>
  <form
    class="space-y-4"
    use:autofocusForm
    onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}
  >
    {#if mode === "create" && projects.length > 1}
      <div class="space-y-1">
        <Label>Project</Label>
        <Select
          items={projects.map(p => ({ value: p.path, label: p.name }))}
          bind:value={formProjectPath}
          placeholder="Select project…"
        />
      </div>
    {/if}

    <div class="space-y-1" data-field="title">
      <Label>Title <span class="font-mono text-[10px] px-1 rounded {badge}">T</span></Label>
      <Input bind:value={formTitle} placeholder="Task title" />
    </div>

    <div class="space-y-1" data-field="desc">
      <Label>Description <span class="font-mono text-[10px] px-1 rounded {badge}">D</span></Label>
      <textarea
        bind:value={formDescription}
        placeholder="Optional description"
        class="w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 resize-none min-h-[4rem] max-h-[50vh] overflow-y-auto focus:outline-none focus:ring-1 focus:ring-accent"
        rows="3"
        oninput={(e) => { const el = e.currentTarget; el.style.height = "auto"; el.style.height = el.scrollHeight + "px"; }}
        use:autoResize
      ></textarea>
    </div>

    <div class="space-y-1" data-field="priority">
      <Label>Priority <span class="font-mono text-[10px] px-1 rounded {badge}">P</span></Label>
      <input type="number" bind:value={formPriority} class="w-20 rounded border border-border bg-panel px-3 py-2 text-sm text-t1 focus:outline-none focus:ring-1 focus:ring-accent" />
    </div>

    <div class="space-y-1" data-field="parent">
      <Label>Parent <span class="font-mono text-[10px] px-1 rounded {badge}">R</span></Label>
      <Select
        items={parentItems}
        bind:value={formParentKey}
        allowDeselect={true}
        placeholder="No parent (top-level)"
        emptyText="No tasks available"
      />
    </div>

    <div class="space-y-1" data-field="blocked">
      <Label>Blocked by <span class="font-mono text-[10px] px-1 rounded {badge}">K</span></Label>
      <PillCombobox
        items={blockerItems}
        bind:values={formBlockedBy}
        placeholder="Search tasks…"
        emptyText="No tasks available"
      />
    </div>

    <div class="space-y-1" data-field="tags">
      <Label>Tags <span class="font-mono text-[10px] px-1 rounded {badge}">G</span></Label>
      <PillInput bind:values={formTags} placeholder="Type and press Enter" />
    </div>

    <div class="space-y-1" data-field="base">
      <Label>Base branch <span class="font-mono text-[10px] px-1 rounded {badge}">B</span></Label>
      <Select
        items={branches}
        bind:value={formBaseBranch}
        placeholder="main"
        emptyText="No branches found"
      />
    </div>

    <div class="sticky bottom-0 bg-panel flex items-center justify-between pt-2 border-t border-border">
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
        <Button type="button" onclick={onCancel}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!formTitle.trim() || submitting}>
          {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}{mode === "create" ? "Create" : "Save"} <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span>{/if}
        </Button>
      </div>
    </div>
  </form>
</div>
