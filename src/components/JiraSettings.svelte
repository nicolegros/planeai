<script lang="ts">
  import { onMount } from "svelte";
  import { jira, projects } from "../lib/api";
  import { getSettings, updateSettings, type AppConfig, type JiraConfig, type JiraProjectMapping, type JiraStatusMapping } from "../lib/settings.svelte";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { Button, Input, Select, Label, Checkbox } from "./ui";
  import type { JiraStatus, SyncResult } from "../lib/types";
  import type { Project } from "../lib/types";

  const config = $derived(getSettings());
  const jiraConfig = $derived(config.integrations?.jira ?? {});
  const mappings = $derived(jiraConfig.project_mappings ?? []);

  let status = $state<JiraStatus>({ connected: false, site: null });
  let connecting = $state(false);
  let syncing = $state(false);
  let projectItems = $state<{ value: string; label: string }[]>([]);

  const planeaiStatuses = [
    { value: "todo", label: "todo" },
    { value: "in_progress", label: "in_progress" },
    { value: "in_review", label: "in_review" },
    { value: "done", label: "done" },
  ];

  onMount(async () => {
    try {
      status = await jira.status();
    } catch (e) {
      showSnackbar(String(e));
    }
    try {
      const projs = await projects.list();
      projectItems = projs.map((p: Project) => ({ value: p.name, label: p.name }));
    } catch (e) {
      showSnackbar(String(e));
    }
  });

  function saveJira(patch: Partial<JiraConfig>) {
    const updated = { ...jiraConfig, ...patch };
    updateSettings({ integrations: { ...config.integrations, jira: updated } } as Partial<AppConfig>);
  }

  function saveMappings(newMappings: JiraProjectMapping[]) {
    saveJira({ project_mappings: newMappings });
  }

  async function connect() {
    connecting = true;
    try {
      await jira.connect();
      status = await jira.status();
      showSnackbar("Connected to Jira");
    } catch (e) {
      showSnackbar(String(e));
    } finally {
      connecting = false;
    }
  }

  async function disconnect() {
    connecting = true;
    try {
      await jira.disconnect();
      status = { connected: false, site: null };
      showSnackbar("Disconnected from Jira");
    } catch (e) {
      showSnackbar(String(e));
    } finally {
      connecting = false;
    }
  }

  async function syncNow() {
    syncing = true;
    try {
      const result: SyncResult = await jira.syncNow();
      showSnackbar(`${result.created} created, ${result.updated} updated, ${result.stale} stale`);
    } catch (e) {
      showSnackbar(String(e));
    } finally {
      syncing = false;
    }
  }

  function addMapping() {
    saveMappings([...mappings, { planeai_project: "", jira_project_key: "", jql_filter: "", status_map: [], writeback: { on_start: "", on_complete: "", comment: false } }]);
  }

  function removeMapping(index: number) {
    saveMappings(mappings.filter((_, i) => i !== index));
  }

  function updateMapping(index: number, patch: Partial<JiraProjectMapping>) {
    const updated = mappings.map((m, i) => i === index ? { ...m, ...patch } : m);
    saveMappings(updated);
  }

  function addStatusPair(index: number) {
    const map = [...(mappings[index].status_map ?? []), { jira_status: "", planeai_status: "todo" }];
    updateMapping(index, { status_map: map });
  }

  function removeStatusPair(mappingIndex: number, pairIndex: number) {
    const map = (mappings[mappingIndex].status_map ?? []).filter((_, i) => i !== pairIndex);
    updateMapping(mappingIndex, { status_map: map });
  }

  function updateStatusPair(mappingIndex: number, pairIndex: number, patch: Partial<JiraStatusMapping>) {
    const map = (mappings[mappingIndex].status_map ?? []).map((p, i) => i === pairIndex ? { ...p, ...patch } : p);
    updateMapping(mappingIndex, { status_map: map });
  }
</script>

<!-- Connection -->
<section class="space-y-3">
  <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Connection</h2>
  <div class="flex items-center gap-3">
    <span class="inline-block w-2.5 h-2.5 rounded-full {status.connected ? 'bg-green-500' : 'bg-surface-400'}"></span>
    <span class="text-sm text-surface-700 dark:text-surface-300">{status.connected ? status.site : 'Not connected'}</span>
  </div>
  <div class="flex gap-2">
    {#if status.connected}
      <Button onclick={disconnect} disabled={connecting} aria-label="Disconnect from Jira">Disconnect</Button>
      <Button onclick={syncNow} disabled={syncing} aria-label="Sync now">{syncing ? 'Syncing…' : 'Sync Now'}</Button>
    {:else}
      <Button onclick={connect} disabled={connecting} aria-label="Connect to Jira">{connecting ? 'Connecting…' : 'Connect to Jira'}</Button>
    {/if}
  </div>
</section>

<!-- Configuration -->
<section class="space-y-3">
  <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Configuration</h2>
  <div class="space-y-1">
    <Label for="jira-site-url">Site URL</Label>
    <Input id="jira-site-url" value={jiraConfig.site_url ?? ""} placeholder="https://mycompany.atlassian.net" onchange={(e) => saveJira({ site_url: e.currentTarget.value || null })} />
  </div>
  <div class="space-y-1">
    <Label for="jira-sync-interval">Sync interval (seconds)</Label>
    <Input id="jira-sync-interval" type="number" value={String((jiraConfig.sync_interval_ms ?? 60000) / 1000)} onchange={(e) => saveJira({ sync_interval_ms: (parseInt(e.currentTarget.value) || 60) * 1000 })} />
  </div>
</section>

<!-- Project Mappings -->
<section class="space-y-3">
  <h2 class="text-sm font-medium text-surface-600 dark:text-surface-300 uppercase tracking-wide">Project Mappings</h2>
  {#each mappings as mapping, i (i)}
    <div class="rounded-lg border border-surface-200 dark:border-surface-700 p-4 space-y-3">
      <div class="flex items-center justify-between">
        <span class="text-sm font-medium text-surface-900 dark:text-surface-50">Mapping {i + 1}</span>
        <button class="text-xs text-red-500 hover:text-red-700" onclick={() => removeMapping(i)} aria-label="Remove mapping {i + 1}">Remove</button>
      </div>

      <div class="space-y-1">
        <Label for="mapping-project-{i}">planeai project</Label>
        <Select items={projectItems} value={mapping.planeai_project} onValueChange={(v) => updateMapping(i, { planeai_project: v })} placeholder="Select project" />
      </div>

      <div class="space-y-1">
        <Label for="mapping-key-{i}">Jira project key</Label>
        <Input id="mapping-key-{i}" value={mapping.jira_project_key} placeholder="PROJ" onchange={(e) => updateMapping(i, { jira_project_key: e.currentTarget.value })} />
      </div>

      <div class="space-y-1">
        <Label for="mapping-jql-{i}">JQL filter</Label>
        <Input id="mapping-jql-{i}" value={mapping.jql_filter ?? ""} placeholder="status != Done" onchange={(e) => updateMapping(i, { jql_filter: e.currentTarget.value || null })} />
      </div>

      <!-- Status Map -->
      <div class="space-y-2">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-xs text-surface-700 dark:text-surface-400">Status map</label>
        {#each mapping.status_map ?? [] as pair, j (j)}
          <div class="flex items-center gap-2">
            <Input value={pair.jira_status} placeholder="Jira status" onchange={(e) => updateStatusPair(i, j, { jira_status: e.currentTarget.value })} class="flex-1" aria-label="Jira status name" />
            <span class="text-xs text-surface-500">→</span>
            <Select items={planeaiStatuses} value={pair.planeai_status} onValueChange={(v) => updateStatusPair(i, j, { planeai_status: v })} class="flex-1" />
            <button class="text-xs text-red-500 hover:text-red-700" onclick={() => removeStatusPair(i, j)} aria-label="Remove status pair">×</button>
          </div>
        {/each}
        <button class="text-xs text-primary-500 hover:text-primary-700" onclick={() => addStatusPair(i)}>+ Add status pair</button>
      </div>

      <!-- Writeback -->
      <div class="space-y-2 pt-2 border-t border-surface-200 dark:border-surface-700">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-xs font-medium text-surface-700 dark:text-surface-300">Writeback</label>
        <div class="grid grid-cols-2 gap-2">
          <div class="space-y-1">
            <Label for="wb-start-{i}">on_start</Label>
            <Input id="wb-start-{i}" value={mapping.writeback?.on_start ?? ""} placeholder="In Progress" onchange={(e) => updateMapping(i, { writeback: { ...mapping.writeback, on_start: e.currentTarget.value || null } })} />
          </div>
          <div class="space-y-1">
            <Label for="wb-complete-{i}">on_complete</Label>
            <Input id="wb-complete-{i}" value={mapping.writeback?.on_complete ?? ""} placeholder="Done" onchange={(e) => updateMapping(i, { writeback: { ...mapping.writeback, on_complete: e.currentTarget.value || null } })} />
          </div>
        </div>
        <Checkbox id="wb-comment-{i}" label="Add comment on transition" checked={mapping.writeback?.comment ?? false} onchange={() => updateMapping(i, { writeback: { ...mapping.writeback, comment: !(mapping.writeback?.comment ?? false) } })} />
      </div>
    </div>
  {/each}

  <button class="px-4 py-2 rounded-md text-sm font-medium bg-surface-200 dark:bg-surface-800 text-surface-700 dark:text-surface-300 hover:bg-surface-300 dark:hover:bg-surface-700" onclick={addMapping}>+ Add Mapping</button>
</section>
