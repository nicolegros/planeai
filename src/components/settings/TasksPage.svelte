<script lang="ts">
  import { getSettings, updateSettings, type AutoDispatchConfig, type TaskManager } from "../../lib/settings.svelte";
  import { settingsWindow } from "./settings-window.svelte";
  import { Switch } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import TextField from "./TextField.svelte";

  type HookKey = "on_start" | "on_notify" | "on_resume" | "on_restart" | "on_complete";

  const VARIABLES = "Variables: {key}, {title}, {status}, {description}, {priority}, {blocked_by}. Transforms: :slug, :lower, :upper.";
  const HOOKS: readonly { key: HookKey; label: string; suggestion: string; description: string }[] = [
    { key: "on_start", label: "On start", suggestion: "in_progress", description: "A session is created from the task." },
    { key: "on_notify", label: "On notify", suggestion: "in_review", description: "The agent signals it is idle." },
    { key: "on_resume", label: "On resume", suggestion: "in_progress", description: "An idle agent with PlaneAI hooks works again. Only moves a task still in the On notify status." },
    { key: "on_restart", label: "On restart", suggestion: "in_progress", description: "An exited task session is restarted." },
    { key: "on_complete", label: "On complete", suggestion: "done", description: "A task session is archived or deleted." },
  ];

  const config = $derived(getSettings());
  const taskManagement = $derived(config.task_management ?? null);

  /** Turning tasks on restores the backend's recommended setup. */
  function enable() {
    const recommended = settingsWindow.defaults?.task_management;
    if (recommended) void updateSettings({ task_management: recommended });
  }

  function patch(next: Partial<TaskManager>) {
    void updateSettings({ task_management: { ...config.task_management, ...next } });
  }

  function setTemplate(field: "branch" | "name" | "prompt", value: string) {
    patch({ templates: { ...taskManagement?.templates, [field]: value || null } });
  }

  function setHook(key: HookKey, value: string) {
    patch({ [key]: value ? { move_to: value } : null });
  }

  function setAutoDispatch(next: Partial<AutoDispatchConfig>) {
    patch({ auto_dispatch: { ...taskManagement?.auto_dispatch, ...next } });
  }
</script>

<SettingsSection title="Task management" help="Link sessions to tasks and move tasks through statuses automatically.">
  <SettingRow id="task-management">
    <Switch
      label="Task management"
      checked={taskManagement !== null}
      disabled={taskManagement === null && !settingsWindow.defaults?.task_management}
      onCheckedChange={(on) => (on ? enable() : updateSettings({ task_management: null }))}
    />
  </SettingRow>
</SettingsSection>

{#if taskManagement}
  <SettingsSection title="Templates" help={VARIABLES}>
    <SettingRow id="branch-template">
      <TextField mono width="w-64" aria-label="Branch name template" value={taskManagement.templates?.branch || "{key:lower}/{title:slug}"} onchange={(e) => setTemplate("branch", e.currentTarget.value)} />
    </SettingRow>
    <SettingRow id="session-name-template">
      <TextField mono width="w-64" aria-label="Session name template" value={taskManagement.templates?.name || "{key:upper}: {title}"} onchange={(e) => setTemplate("name", e.currentTarget.value)} />
    </SettingRow>
    <SettingRow id="prompt-template" stacked>
      <textarea
        aria-label="Initial prompt template"
        class="min-h-20 w-full resize-y rounded-md border border-border-s bg-panel px-2 py-1.5 font-mono text-[12px] text-t1 focus:outline-none focus:ring-1 focus:ring-accent"
        value={taskManagement.templates?.prompt || "Implement task {key}: {title}\n\n{description}"}
        onchange={(e) => setTemplate("prompt", e.currentTarget.value)}
      ></textarea>
    </SettingRow>
  </SettingsSection>

  <SettingsSection title="Status transitions" help="Move the linked task to a status when its session reaches a stage. Leave empty to disable.">
    <div id="setting-lifecycle-hooks" data-setting-id="lifecycle-hooks" class="divide-y divide-border">
      {#each HOOKS as hook (hook.key)}
        <div class="flex items-center justify-between gap-6 px-4 py-2.5">
          <div class="min-w-0">
            <label class="text-[13px] text-t1" for="hook-{hook.key}">{hook.label}</label>
            <p class="text-[12px] leading-snug text-t3">{hook.description}</p>
          </div>
          <!-- An unset hook never fires, so its suggestion is only a placeholder. -->
          <TextField
            id="hook-{hook.key}"
            mono
            width="w-44"
            value={taskManagement[hook.key]?.move_to ?? ""}
            placeholder={`Disabled - e.g. ${hook.suggestion}`}
            onchange={(e) => setHook(hook.key, e.currentTarget.value.trim())}
          />
        </div>
      {/each}
    </div>
  </SettingsSection>

  <SettingsSection title="Auto-dispatch" help="Start agent sessions for tasks automatically.">
    <SettingRow id="auto-dispatch" badge="Experimental">
      <Switch
        label="Auto-dispatch"
        checked={!!taskManagement.auto_dispatch}
        onCheckedChange={(on) => patch({ auto_dispatch: on ? { poll_interval_ms: 30000, max_concurrent: 3 } : null })}
      />
    </SettingRow>
    {#if taskManagement.auto_dispatch}
      {@const dispatch = taskManagement.auto_dispatch}
      {#snippet field(id: string, label: string, help: string, value: string, placeholder: string, onCommit: (value: string) => void, width = "w-44")}
        <div class="flex items-center justify-between gap-6 px-4 py-2.5">
          <div class="min-w-0">
            <label class="text-[13px] text-t1" for={id}>{label}</label>
            <p class="text-[12px] leading-snug text-t3">{help}</p>
          </div>
          <TextField {id} mono {width} {value} {placeholder} onchange={(e) => onCommit(e.currentTarget.value)} />
        </div>
      {/snippet}
      {@render field("dispatch-poll", "Poll interval", "Milliseconds between checks for new tasks.", String(dispatch.poll_interval_ms ?? 30000), "30000", (v) => setAutoDispatch({ poll_interval_ms: Number.parseInt(v, 10) || 30000 }), "w-24")}
      {@render field("dispatch-max", "Max concurrent sessions", "Dispatching pauses while this many are running.", String(dispatch.max_concurrent ?? 3), "3", (v) => setAutoDispatch({ max_concurrent: Number.parseInt(v, 10) || 3 }), "w-24")}
      {@render field("dispatch-provider", "Agent", "Leave empty to use the default agent.", dispatch.provider || "", config.default_provider, (v) => setAutoDispatch({ provider: v.trim() || undefined }))}
      {@render field("dispatch-terminal", "Terminal states", "Comma-separated. Tasks in these states are never dispatched, and their running sessions are stopped.", (dispatch.terminal_states ?? ["done", "cancelled"]).join(", "), "done, cancelled", (v) => setAutoDispatch({ terminal_states: v.split(",").map((s) => s.trim()).filter(Boolean) }))}
      <div class="space-y-1.5 px-4 py-2.5">
        <label class="text-[13px] text-t1" for="dispatch-template">Autonomous prompt template</label>
        <p class="text-[12px] leading-snug text-t3">Wraps the task prompt for dispatched sessions; {"{prompt}"} is replaced with it. Leave empty to send the task prompt as-is.</p>
        <textarea
          id="dispatch-template"
          class="min-h-16 w-full resize-y rounded-md border border-border-s bg-panel px-2 py-1.5 font-mono text-[12px] text-t1 placeholder:text-t3 focus:outline-none focus:ring-1 focus:ring-accent"
          value={dispatch.autonomous_prompt_template || ""}
          placeholder={"e.g. Be autonomous.\n{prompt}"}
          onchange={(e) => setAutoDispatch({ autonomous_prompt_template: e.currentTarget.value || null })}
        ></textarea>
      </div>
    {/if}
  </SettingsSection>
{/if}
