<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { CircleCheck, CircleAlert, Folder, Pencil, Plus, Trash2, X } from "@lucide/svelte";
  import { config as configApi } from "../../lib/api";
  import { showSnackbar } from "../../lib/snackbar.svelte";
  import { getSettings, type AppConfig } from "../../lib/settings.svelte";
  import { addCustomAgentPatch, agentRows, disableAgentPatch, enableAgentPatch, setDefaultAgentPatch, type AgentRow } from "../../lib/agent-settings";
  import { Button, Select, Switch } from "../ui";
  import AgentDialog, { type AgentDialogResult } from "./AgentDialog.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingAnchor from "./SettingAnchor.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import { saveSettings } from "../../lib/save-settings";

  const config = $derived(getSettings());
  let detected = $state<Record<string, string | null>>({});
  const rows = $derived(agentRows(config, detected));
  const enabledRows = $derived(rows.filter((row) => row.enabled));
  const searchPaths = $derived(config.extra_path_dirs ?? []);

  let dialog = $state<{ editing: AgentRow | null } | null>(null);
  let dialogError = $state("");

  let detectRequest = 0;

  async function detect() {
    const request = ++detectRequest;
    try {
      const result = await configApi.detectProviders();
      // Detections can finish out of order; only the latest reflects the current config.
      if (request === detectRequest) detected = result;
    } catch (error) {
      console.warn("Agent detection failed:", error);
    }
  }

  $effect(() => {
    void detect();
  });

  /** Detection reads the backend's config, so it must run after the update lands. */
  async function apply(patch: Partial<AppConfig> | null) {
    if (!patch) return;
    await saveSettings(patch);
    await detect();
  }

  function toggle(row: AgentRow, enabled: boolean) {
    void apply(enabled ? enableAgentPatch(config, row.key) : disableAgentPatch(config, row.key));
  }

  function openDialog(editing: AgentRow | null) {
    dialogError = "";
    dialog = { editing };
  }

  function saveDialog(result: AgentDialogResult) {
    const editing = dialog?.editing;
    if (editing) {
      if (!result.provider.command) {
        dialogError = "Command is required.";
        return;
      }
      void apply({ providers: { ...config.providers, [editing.key]: result.provider } });
    } else {
      const added = addCustomAgentPatch(config, { name: result.name, command: result.provider.command, yoloFlag: result.provider.yolo_flag ?? "" });
      if (!added.ok) {
        dialogError = added.error;
        return;
      }
      void apply(added.patch);
    }
    dialog = null;
  }

  async function addSearchPath() {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string" || searchPaths.includes(selected)) return;
      await apply({ extra_path_dirs: [...searchPaths, selected] });
    } catch (error) {
      showSnackbar(`Failed to add a search path: ${error}`, "error");
    }
  }

  function removeSearchPath(path: string) {
    void apply({ extra_path_dirs: searchPaths.filter((candidate) => candidate !== path) });
  }
</script>

<SettingsSection title="Default">
  <SettingRow id="default-agent">
    <div class="w-48">
      <Select
        items={enabledRows.map((row) => ({ value: row.key, label: row.label }))}
        value={config.default_provider}
        onValueChange={(key) => apply(setDefaultAgentPatch(config, key))}
        ariaLabel="Default agent"
      />
    </div>
  </SettingRow>
</SettingsSection>

<SettingsSection title="Agents" help="Built-in agents are always listed; turn on the ones you use. Agents not found on your PATH can still be turned on." bare>
  <SettingAnchor id="agents" class="divide-y divide-border overflow-hidden rounded-xl border border-border-s bg-panel">
    {#each rows as row (row.key)}
      <div class="flex items-center gap-3 px-4 py-2.5" data-agent={row.key}>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2 text-[13px] text-t1">
            {row.label}
            {#if row.isDefault}<span class="rounded bg-accent-bg px-1.5 py-px text-[10px] font-medium text-t2">Default</span>{/if}
            {#if row.custom}<span class="text-[11px] text-t3">Custom</span>{/if}
          </div>
          <div class="mt-0.5 flex min-w-0 items-center gap-1.5 text-[11px]">
            {#if row.detection.state === "found"}
              <CircleCheck size={11} class="shrink-0 text-status-running" />
              <span class="truncate font-mono text-t3" title={row.detection.path}>{row.detection.path}</span>
            {:else if row.detection.state === "missing"}
              <CircleAlert size={11} class="shrink-0 text-status-review" />
              <span class="text-t3">Not found on PATH</span>
            {/if}
            {#if row.provider && row.detection.state !== "found"}
              <span class="truncate font-mono text-t3">{row.provider.command}</span>
            {/if}
          </div>
        </div>
        {#if row.provider}
          <button type="button" class="grid h-6 w-6 place-items-center rounded text-t3 hover:bg-panel-hi hover:text-t1" aria-label="Edit {row.label}" onclick={() => openDialog(row)}><Pencil size={12} /></button>
        {/if}
        {#if row.custom}
          <button
            type="button"
            class="grid h-6 w-6 place-items-center rounded text-t3 hover:bg-panel-hi hover:text-status-exited disabled:pointer-events-none disabled:opacity-40"
            aria-label="Delete {row.label}"
            title={row.lockedReason ?? undefined}
            disabled={row.lockedReason !== null}
            onclick={() => toggle(row, false)}
          ><Trash2 size={12} /></button>
        {:else}
          <Switch
            label="Enable {row.label}"
            checked={row.enabled}
            disabled={row.lockedReason !== null}
            title={row.lockedReason ?? undefined}
            onCheckedChange={(enabled) => toggle(row, enabled)}
          />
        {/if}
      </div>
    {/each}
    <button type="button" class="flex w-full items-center gap-2 px-4 py-2.5 text-[12px] text-t2 hover:bg-panel-hi hover:text-t1" onclick={() => openDialog(null)}>
      <Plus size={13} />Add custom agent
    </button>
  </SettingAnchor>
</SettingsSection>

<SettingsSection title="Search paths">
  <SettingRow id="search-paths" stacked>
    {#if searchPaths.length > 0}
      <ul class="mb-2 divide-y divide-border overflow-hidden rounded-md border border-border-s">
        {#each searchPaths as path (path)}
          <li class="flex items-center gap-2 px-2.5 py-1.5">
            <Folder size={12} class="shrink-0 text-t3" />
            <span class="min-w-0 flex-1 truncate font-mono text-[12px] text-t1">{path}</span>
            <button type="button" class="grid h-5 w-5 place-items-center rounded text-t3 hover:bg-panel-hi hover:text-t1" aria-label="Remove {path}" onclick={() => removeSearchPath(path)}><X size={11} /></button>
          </li>
        {/each}
      </ul>
    {/if}
    <Button type="button" size="sm" onclick={addSearchPath}>Add folder…</Button>
  </SettingRow>
</SettingsSection>

{#if dialog}
  <AgentDialog
    editing={dialog.editing?.provider ? { key: dialog.editing.key, label: dialog.editing.label, provider: dialog.editing.provider } : null}
    error={dialogError}
    onSave={saveDialog}
    onClose={() => (dialog = null)}
  />
{/if}
