<script lang="ts">
  import { onMount } from "svelte";
  import { jira, projects } from "../lib/api";
  import { getSettings, updateSettings, type JiraConfig, type JiraProjectMapping, type IntegrationsConfig } from "../lib/settings.svelte";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { Button, Input, Select, Label, Checkbox } from "./ui";
  import type { JiraStatus, SyncResult } from "../lib/types";
  import type { Project } from "../lib/types";

  const config = $derived(getSettings());
  const jiraConfig = $derived(config.integrations?.jira ?? null);
  const projectsMap = $derived(jiraConfig?.projects ?? {});

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
    const [statusResult, projectsResult] = await Promise.allSettled([
      jira.status(),
      projects.list(),
    ]);
    if (statusResult.status === "fulfilled") status = statusResult.value;
    else showSnackbar(String(statusResult.reason));
    if (projectsResult.status === "fulfilled") projectItems = projectsResult.value.map((p: Project) => ({ value: p.name, label: p.name }));
    else showSnackbar(String(projectsResult.reason));
  });

  function saveJira(patch: Partial<JiraConfig>) {
    const current = jiraConfig ?? { site: "" };
    const updated: JiraConfig = { ...current, ...patch };
    const integrations: IntegrationsConfig = { ...config.integrations, jira: updated };
    updateSettings({ integrations });
  }

  async function connect() {
    if (!jiraConfig?.site) {
      showSnackbar("Enter a Jira site URL first");
      return;
    }
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

  function addProject() {
    const key = `project_${Object.keys(projectsMap).length + 1}`;
    saveJira({ projects: { ...projectsMap, [key]: { jira_project: "", jql: "", status_map: {}, writeback: null } } });
  }

  function removeProject(key: string) {
    const updated = { ...projectsMap };
    delete updated[key];
    saveJira({ projects: updated });
  }

  function updateProject(key: string, patch: Partial<JiraProjectMapping>) {
    saveJira({ projects: { ...projectsMap, [key]: { ...projectsMap[key], ...patch } } });
  }

  function renameProjectKey(oldKey: string, newKey: string) {
    if (!newKey || newKey === oldKey) return;
    const updated = { ...projectsMap };
    updated[newKey] = updated[oldKey];
    delete updated[oldKey];
    saveJira({ projects: updated });
  }
</script>

<!-- Connection -->
<section class="space-y-3">
  <h2 class="text-sm font-medium text-t3 uppercase tracking-wide">Connection</h2>
  <div class="flex items-center gap-3">
    <span class="inline-block w-2.5 h-2.5 rounded-full {status.connected ? 'bg-green-500' : 'bg-surface-400'}"></span>
    <span class="text-sm text-t2">{status.connected ? status.site : 'Not connected'}</span>
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
  <h2 class="text-sm font-medium text-t3 uppercase tracking-wide">Configuration</h2>
  <div class="space-y-1">
    <Label for="jira-site">Site URL</Label>
    <Input id="jira-site" value={jiraConfig?.site ?? ""} placeholder="https://mycompany.atlassian.net" onchange={(e) => saveJira({ site: e.currentTarget.value })} />
  </div>
  <div class="space-y-1">
    <Label for="jira-sync-interval">Sync interval (seconds)</Label>
    <Input id="jira-sync-interval" type="number" value={String((jiraConfig?.sync_interval_ms ?? 60000) / 1000)} onchange={(e) => saveJira({ sync_interval_ms: (parseInt(e.currentTarget.value) || 60) * 1000 })} />
  </div>
</section>

<!-- Project Mappings -->
<section class="space-y-3">
  <h2 class="text-sm font-medium text-t3 uppercase tracking-wide">Project Mappings</h2>
  {#each Object.entries(projectsMap) as [key, mapping], i (key)}
    <div class="rounded-lg border border-border p-4 space-y-3">
      <div class="flex items-center justify-between">
        <span class="text-sm font-medium text-t1">Mapping: {key}</span>
        <button class="text-xs text-red-500 hover:text-red-700" onclick={() => removeProject(key)} aria-label="Remove mapping {key}">Remove</button>
      </div>

      <div class="space-y-1">
        <Label>planeai project</Label>
        <Select items={projectItems} value={key} onValueChange={(v) => renameProjectKey(key, v)} placeholder="Select project" />
      </div>

      <div class="space-y-1">
        <Label for="mapping-jira-{i}">Jira project key</Label>
        <Input id="mapping-jira-{i}" value={mapping.jira_project} placeholder="PROJ" onchange={(e) => updateProject(key, { jira_project: e.currentTarget.value })} />
      </div>

      <div class="space-y-1">
        <Label for="mapping-jql-{i}">JQL filter</Label>
        <Input id="mapping-jql-{i}" value={mapping.jql ?? ""} placeholder="status != Done" onchange={(e) => updateProject(key, { jql: e.currentTarget.value })} />
      </div>

      <!-- Status Map -->
      <div class="space-y-2">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-xs text-t3">Status map (Jira status → planeai status)</label>
        {#each Object.entries(mapping.status_map ?? {}) as [jiraStatus, planeaiStatus], j (jiraStatus)}
          <div class="flex items-center gap-2">
            <Input value={jiraStatus} placeholder="Jira status" onchange={(e) => {
              const map = { ...mapping.status_map };
              const val = map[jiraStatus];
              delete map[jiraStatus];
              if (e.currentTarget.value) map[e.currentTarget.value] = val;
              updateProject(key, { status_map: map });
            }} class="flex-1" aria-label="Jira status name" />
            <span class="text-xs text-t3">→</span>
            <Select items={planeaiStatuses} value={planeaiStatus} onValueChange={(v) => {
              updateProject(key, { status_map: { ...mapping.status_map, [jiraStatus]: v } });
            }} class="flex-1" />
            <button class="text-xs text-red-500 hover:text-red-700" onclick={() => {
              const map = { ...mapping.status_map };
              delete map[jiraStatus];
              updateProject(key, { status_map: map });
            }} aria-label="Remove status pair">×</button>
          </div>
        {/each}
        <button class="text-xs text-accent hover:underline" onclick={() => {
          updateProject(key, { status_map: { ...mapping.status_map, "": "todo" } });
        }}>+ Add status pair</button>
      </div>

      <!-- Writeback -->
      <div class="space-y-2 pt-2 border-t border-border">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label class="text-xs font-medium text-t2">Writeback</label>
        <div class="grid grid-cols-2 gap-2">
          <div class="space-y-1">
            <Label for="wb-start-{i}">on_start</Label>
            <Input id="wb-start-{i}" value={mapping.writeback?.on_start ?? ""} placeholder="In Progress" onchange={(e) => updateProject(key, { writeback: { ...mapping.writeback, on_start: e.currentTarget.value || null } })} />
          </div>
          <div class="space-y-1">
            <Label for="wb-complete-{i}">on_complete</Label>
            <Input id="wb-complete-{i}" value={mapping.writeback?.on_complete ?? ""} placeholder="Done" onchange={(e) => updateProject(key, { writeback: { ...mapping.writeback, on_complete: e.currentTarget.value || null } })} />
          </div>
        </div>
        <Checkbox id="wb-comment-{i}" label="Add comment on transition" checked={mapping.writeback?.comment ?? false} onchange={() => updateProject(key, { writeback: { ...mapping.writeback, comment: !(mapping.writeback?.comment ?? false) } })} />
      </div>
    </div>
  {/each}

  <button class="px-4 py-2 rounded-md text-sm font-medium bg-surface-200 dark:bg-surface-800 text-t2 hover:bg-surface-300 dark:hover:bg-surface-700" onclick={addProject}>+ Add Mapping</button>
</section>
