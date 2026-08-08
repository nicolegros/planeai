<script lang="ts">
  import type { Project, TaskItem, Session } from "../lib/types";
  import { sessions as sessionsApi, projects as projectsApi } from "../lib/api";
  import { Button, Input, Label, Select, PillInput, PillCombobox, Checkbox } from "./ui";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { renderTemplate } from "../lib/render-template";
  import { LoaderCircle } from "@lucide/svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    mode: "create" | "edit";
    projects: Project[];
    tasks?: TaskItem[];
    sessions?: Session[];
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
    onSessionCreated?: (session: Session) => void;
  }

  let { mode, projects, tasks = [], sessions = [], initial = {}, onSubmitted, onCancel, onSessionCreated }: Props = $props();

  const config = $derived(getSettings());
  const providerKeys = $derived(Object.keys(config.providers ?? {}));

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

  // ─── Start session toggle ───────────────────────────────────────────────────
  // svelte-ignore state_referenced_locally
  let startSession = $state(mode === "create");
  let sessionProvider = $state("");
  let sessionBranch = $state("");
  let sessionPrompt = $state("");
  let useWorktree = $state(true);
  let autoApprove = $state(true);

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

  // Auto-generate session fields from task title/description
  const slugFromTitle = $derived(
    formTitle.toLowerCase().replace(/\s+/g, "-").replace(/[^a-z0-9\-/]/g, "").replace(/-+$/, "")
  );

  // Template rendering for prompt preview
  // Build a preview of what the prompt will be using configured templates
  const defaultPrompt = $derived.by(() => {
    const templates = config.task_management?.templates;
    const virtualTask = {
      key: "TASK-?",
      title: formTitle.trim(),
      description: formDescription,
      priority: formPriority,
      blocked_by: formBlockedBy,
      tags: formTags,
      parent_key: formParentKey,
      base_branch: formBaseBranch,
      status: "todo",
    };
    if (templates?.prompt) {
      return renderTemplate(templates.prompt, virtualTask);
    }
    return formDescription
      ? `Implement task ${virtualTask.key}: ${virtualTask.title}\n\n${formDescription}`
      : `Implement task ${virtualTask.key}: ${virtualTask.title}`;
  });

  // Derive task items for parent_key and blocked_by comboboxes — scoped to current project
  const projectTasks = $derived.by(() => {
    const fromStore = taskStore.getTasksForProject(formProjectPath);
    const pool = fromStore.length > 0 ? fromStore : tasks;
    const seen = new Set<string>();
    return pool.filter((t) => {
      if (seen.has(t.key)) return false;
      seen.add(t.key);
      return true;
    });
  });

  const parentItems = $derived(
    projectTasks
      .filter((t) => t.key !== initial.key)
      .map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` }))
  );

  const blockerItems = $derived(
    projectTasks
      .filter((t) => t.key !== initial.key)
      .map((t) => ({ value: t.key, label: `${t.key}: ${t.title}` }))
  );

  const selectedProject = $derived(projects.find((p) => p.path === formProjectPath));

  // Check if branch is already used by an active session
  const branchAlreadyUsed = $derived(
    startSession && sessionBranch && selectedProject &&
    sessions.some(s => s.project_id === selectedProject.id && s.status === "active" && s.branch === sessionBranch && !s.worktree_path)
  );

  const fk = createFormKeyboardController(
    () => [
      { key: "o", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='project'] input") ?? null },
      { key: "t", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='title'] input") ?? null },
      { key: "d", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='desc'] textarea") ?? null },
      { key: "r", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='priority'] input") ?? null },
      { key: "a", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='parent'] input") ?? null },
      { key: "k", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='blocked'] input") ?? null },
      { key: "g", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='tags'] input") ?? null },
      { key: "b", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='base'] input") ?? null },
      ...(mode === "create" ? [
        { key: "s", toggle: () => { startSession = !startSession; } },
      ] : []),
      ...(startSession ? [
        { key: "p", toggle: () => { const current = sessionProvider || config.default_provider; const idx = providerKeys.indexOf(current); sessionProvider = providerKeys[(idx + 1) % providerKeys.length]; }, shiftToggle: () => { const current = sessionProvider || config.default_provider; const idx = providerKeys.indexOf(current); sessionProvider = providerKeys[(idx - 1 + providerKeys.length) % providerKeys.length]; } },
        { key: "w", toggle: () => { useWorktree = !useWorktree; } },
        { key: "y", toggle: () => { autoApprove = !autoApprove; } },
        { key: "n", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='session-branch'] input") ?? null },
        { key: "i", ref: () => formWrapper?.querySelector<HTMLElement>("[data-field='session-prompt'] textarea") ?? null },
      ] : []),
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
        const createdTask = await taskStore.createTask({
          repoPath,
          title: formTitle.trim(),
          description: formDescription,
          priority: formPriority,
          tags: formTags,
          blockedBy: formBlockedBy,
          parentKey: formParentKey || null,
          baseBranch: formBaseBranch,
        });

        if (startSession && selectedProject) {
          // Validate provider
          const provider = sessionProvider || config.default_provider;
          if (!provider) {
            showSnackbar("Task created, but no provider configured. Select a provider to start a session.");
            onSubmitted();
            return;
          }

          // Build session params from templates
          const templates = config.task_management?.templates;
          const realTask = {
            key: createdTask.key,
            title: createdTask.title,
            description: createdTask.description,
            priority: createdTask.priority,
            blocked_by: createdTask.blocked_by,
            tags: createdTask.tags,
            parent_key: createdTask.parent_key,
            base_branch: createdTask.base_branch,
            status: createdTask.status,
          };
          const branch = sessionBranch || (templates?.branch
            ? renderTemplate(templates.branch, realTask)
            : `${createdTask.key.toLowerCase()}/${slugFromTitle}`);
          const isNewBranch = !branches.some((b) => b.value === branch);
          const prompt = sessionPrompt || (templates?.prompt
            ? renderTemplate(templates.prompt, realTask)
            : (createdTask.description ? `Implement task ${createdTask.key}: ${createdTask.title}\n\n${createdTask.description}` : `Implement task ${createdTask.key}: ${createdTask.title}`));
          const name = templates?.name
            ? renderTemplate(templates.name, realTask)
            : `${createdTask.key}: ${formTitle.trim()}`;

          try {
            const { session, warning } = await sessionsApi.launch({
              projectId: selectedProject.id,
              projectName: selectedProject.name,
              repoPath: selectedProject.path,
              branch,
              isNewBranch,
              name,
              useWorktree,
              baseBranch: isNewBranch ? formBaseBranch : null,
              autoApprove,
              provider,
              taskKey: createdTask.key,
              taskPrompt: prompt,
            });
            if (warning) showSnackbar(warning, "success");
            // Move to in_progress — don't block session navigation on failure
            taskStore.moveTask(createdTask.key, "in_progress", repoPath).catch(() => {
              showSnackbar("Session started but failed to update task status.");
            });
            onSessionCreated?.(session);
          } catch (e: any) {
            showSnackbar(`Task created but session failed: ${e}`);
          }
        }
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
    } finally {
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
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div bind:this={formWrapper} tabindex="-1" onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); handleSubmit(); return; } fk.handleKeydown(e); }} onfocusin={fk.handleFocusin} class="outline-none px-5 pb-5" data-form-keyboard>
  <form
    class="space-y-4"
    use:autofocusForm
    onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}
  >
    {#if mode === "create" && projects.length > 1}
      <div class="space-y-1" data-field="project">
        <Label>Project <span class="font-mono text-[10px] px-1 rounded {badge}">O</span></Label>
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
      <Label>Priority <span class="font-mono text-[10px] px-1 rounded {badge}">R</span></Label>
      <input type="number" bind:value={formPriority} class="w-20 rounded border border-border bg-panel px-3 py-2 text-sm text-t1 focus:outline-none focus:ring-1 focus:ring-accent" />
    </div>

    <div class="space-y-1" data-field="parent">
      <Label>Parent <span class="font-mono text-[10px] px-1 rounded {badge}">A</span></Label>
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

    <!-- Start session toggle (create mode only) -->
    {#if mode === "create"}
      <div class="border-t border-border pt-4 mt-4">
        <div class="flex items-center gap-2">
          <Checkbox id="start-session" label="Start session immediately" bind:checked={startSession} />
          <span class="font-mono text-[10px] px-1 rounded {badge}">S</span>
        </div>
      </div>

      {#if startSession}
        <div class="space-y-3 pl-1 border-l-2 border-accent/30 ml-1">
          <!-- Provider -->
          {#if providerKeys.length > 1}
            <div class="space-y-1 pl-3">
              <Label>Provider <span class="font-mono text-[10px] px-1 rounded {badge}">P</span></Label>
              <Select
                items={providerKeys.map(k => ({ value: k, label: k }))}
                bind:value={sessionProvider}
                placeholder={config.default_provider ?? "Select provider…"}
              />
            </div>
          {:else}
            <p class="text-xs text-t3 pl-3">Provider: <span class="font-medium text-t1">{config.default_provider}</span> <span class="font-mono text-[10px] px-1 rounded {badge}">P</span></p>
          {/if}

          <!-- Worktree & Auto-approve -->
          <div class="flex items-center gap-4 pl-3">
            <Checkbox id="use-worktree" label="Worktree" bind:checked={useWorktree} tabindex={-1} />
            <span class="font-mono text-[10px] px-1 rounded {badge}">W</span>
            <Checkbox id="auto-approve" label="Auto-approve" bind:checked={autoApprove} tabindex={-1} />
            <span class="font-mono text-[10px] px-1 rounded {badge}">Y</span>
          </div>

          <!-- Branch -->
          <div class="space-y-1 pl-3" data-field="session-branch">
            <Label>Branch <span class="font-mono text-[10px] px-1 rounded {badge}">N</span></Label>
            <Input
              bind:value={sessionBranch}
              placeholder={slugFromTitle ? `${slugFromTitle}` : "auto-generated from title"}
            />
            {#if sessionBranch || slugFromTitle}
              <p class="text-xs text-t3">Will create: <span class="font-medium font-mono text-t1">{sessionBranch || slugFromTitle}</span> from <span class="font-medium font-mono text-t1">{formBaseBranch || "main"}</span></p>
            {/if}
          </div>

          {#if branchAlreadyUsed}
            <p class="text-xs text-status-review pl-3">Another session is using this branch — switching branches will affect it.</p>
          {/if}

          <!-- Initial prompt -->
          <div class="space-y-1 pl-3" data-field="session-prompt">
            <Label>Initial prompt <span class="font-mono text-[10px] px-1 rounded {badge}">I</span></Label>
            <textarea
              bind:value={sessionPrompt}
              placeholder={defaultPrompt}
              class="w-full rounded border border-border bg-panel px-3 py-2 text-sm text-t1 placeholder:text-t3 resize-none min-h-[3rem] max-h-[30vh] overflow-y-auto focus:outline-none focus:ring-1 focus:ring-accent"
              rows="2"
              oninput={(e) => { const el = e.currentTarget; el.style.height = "auto"; el.style.height = el.scrollHeight + "px"; }}
            ></textarea>
            <p class="text-[11px] text-t3">Leave empty to use the default prompt shown above.</p>
          </div>
        </div>
      {/if}
    {/if}

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
          {#if submitting}<LoaderCircle class="size-3.5 animate-spin" />{:else}{mode === "create" && startSession ? "Create & Start" : mode === "create" ? "Create" : "Save"} <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span>{/if}
        </Button>
      </div>
    </div>
  </form>
</div>
