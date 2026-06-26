<script lang="ts">
  import type { TaskItem, Project } from "../lib/types";
  import { jira as jiraApi } from "../lib/api";
  import { isPlatformMod, MOD_ENTER_HINT } from "../lib/keyboard";
  import { createFormKeyboardController } from "../lib/form-keyboard.svelte";
  import { Select, Button, Label } from "./ui";
  import FormDialog from "./ui/FormDialog.svelte";
  import * as jiraTaskStore from "../lib/jira-task-store.svelte";
  import * as taskStore from "../lib/task-store.svelte";

  interface Props {
    task: TaskItem;
    projects: Project[];
    preselectedProjectId?: string;
    onClose: () => void;
    onNewProject: () => void;
  }

  let { task, projects, preselectedProjectId = "", onClose, onNewProject }: Props = $props();

  let projectId = $state(preselectedProjectId);
  let error = $state("");
  let wrapper = $state<HTMLDivElement | null>(null);

  const fk = createFormKeyboardController(
    () => [
      { key: "p", ref: () => wrapper?.querySelector<HTMLElement>("[data-field='project'] input") ?? null },
      { key: "n", toggle: () => onNewProject() },
    ],
    { wrapper: () => wrapper, onDismiss: onClose },
  );

  async function submit() {
    if (!projectId) return;
    try {
      await jiraApi.assign(task.key, projectId);
      onClose();
      await jiraTaskStore.loadJiraTasks();
      await taskStore.refresh(projects.map(p => p.path));
    } catch (e) {
      error = String(e);
    }
  }
</script>

<FormDialog title="Assign to project" {onClose}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div bind:this={wrapper} tabindex="-1" onkeydown={fk.handleKeydown} onfocusin={fk.handleFocusin} class="outline-none px-5 pb-5" data-form-keyboard>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form class="space-y-4" onsubmit={(e) => { e.preventDefault(); submit(); }} onkeydown={(e) => { if (e.key === "Enter" && isPlatformMod(e)) { e.preventDefault(); submit(); } }}>
      <div class="flex items-start justify-between">
        <p class="text-sm text-t2">Assign <span class="font-mono font-medium text-t1">{task.key}</span> to a project:</p>
        <Button type="button" onclick={onNewProject}>New project <span class="font-mono text-[10px] px-1 rounded {fk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">N</span></Button>
      </div>
      <div class="space-y-1" data-field="project">
        <Label>Project <span class="font-mono text-[10px] px-1 rounded {fk.mode === 'normal' ? 'bg-accent-bg text-accent' : 'bg-panel-hi text-t3'}">P</span></Label>
        <Select
          items={projects.map(p => ({ value: p.id, label: p.name }))}
          bind:value={projectId}
          placeholder="Select project…"
        />
      </div>
      {#if error}<p class="text-xs text-status-exited">{error}</p>{/if}
      <div class="flex items-center justify-between pt-2 border-t border-border">
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
          <Button type="button" onclick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!projectId}>Assign <span class="ml-1 text-xs opacity-60">{MOD_ENTER_HINT}</span></Button>
        </div>
      </div>
    </form>
  </div>
</FormDialog>
