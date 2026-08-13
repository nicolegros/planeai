<script lang="ts">
  import { onMount } from "svelte";
  import { plugins } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { Button, Input, Label } from "./ui";

  interface JiraSettings { site: string; sync_interval_ms: number; }
  interface JiraStatus { connected: boolean; site: string | null; }

  let settings = $state<JiraSettings>({ site: "", sync_interval_ms: 60000 });
  let status = $state<JiraStatus>({ connected: false, site: null });
  let loading = $state(true);
  let connecting = $state(false);
  const hasSite = $derived(!!settings.site.trim());

  async function call<T>(method: string, params: unknown = null): Promise<T> {
    return plugins.call<T>("jira", method, params);
  }

  async function refresh(): Promise<void> {
    const [stored, nextStatus] = await Promise.all([
      plugins.settings<Partial<JiraSettings>>("jira"),
      call<JiraStatus>("jira.status"),
    ]);
    settings = {
      site: stored.site ?? "",
      sync_interval_ms: stored.sync_interval_ms ?? 60000,
    };
    status = nextStatus;
  }

  onMount(async () => {
    try {
      await plugins.enable("jira");
      await refresh();
    } catch (error) {
      showSnackbar(`Jira plugin could not start: ${String(error)}`, "error");
    } finally {
      loading = false;
    }
  });

  async function save(): Promise<void> {
    try {
      settings = await plugins.updateSettings<JiraSettings>("jira", settings);
    } catch (error) {
      showSnackbar(`Could not save Jira settings: ${String(error)}`, "error");
    }
  }

  async function connect(): Promise<void> {
    connecting = true;
    try {
      await save();
      const { authorization_url } = await call<{ authorization_url: string }>("jira.connect.start");
      try {
        await plugins.openJiraAuthorizationUrl(authorization_url);
      } catch (error) {
        await call("jira.connect.cancel").catch(() => {});
        throw new Error(`Could not open your browser. ${String(error)}`);
      }
      showSnackbar("Finish Jira authorization in your browser…", "success");
      await call("jira.connect.complete");
      await refresh();
      showSnackbar("Connected to Jira", "success");
    } catch (error) {
      await refresh().catch(() => {});
      showSnackbar(`Jira connection failed: ${String(error)}`, "error");
    } finally {
      connecting = false;
    }
  }

  async function disconnect(): Promise<void> {
    connecting = true;
    try {
      await call("jira.disconnect");
      await refresh();
      showSnackbar("Disconnected from Jira", "success");
    } catch (error) {
      showSnackbar(`Could not disconnect Jira: ${String(error)}`, "error");
    } finally {
      connecting = false;
    }
  }
</script>

<section class="space-y-4" aria-busy={loading}>
  <div class="space-y-1">
    <h2 class="text-sm font-medium text-t3 uppercase tracking-wide">Jira</h2>
    <p class="text-xs text-t3">Connection settings are stored only for the bundled Jira plugin.</p>
  </div>

  <div class="flex items-center gap-3" aria-live="polite">
    <span class="inline-block w-2.5 h-2.5 rounded-full {status.connected ? 'bg-status-running' : 'bg-surface-400'}"></span>
    <span class="text-sm text-t2">{status.connected ? `Connected to ${status.site ?? settings.site}` : 'Not connected'}</span>
    {#if status.connected}
      <Button onclick={disconnect} disabled={connecting}>{connecting ? 'Disconnecting…' : 'Disconnect'}</Button>
    {:else}
      <Button onclick={connect} disabled={connecting || !hasSite}>{connecting ? 'Connecting…' : 'Connect'}</Button>
      {#if !hasSite}<span class="text-xs text-t3">Enter a site URL to connect</span>{/if}
    {/if}
  </div>

  <div class="space-y-1">
    <Label for="jira-site">Site URL</Label>
    <Input id="jira-site" bind:value={settings.site} placeholder="https://mycompany.atlassian.net" onchange={save} />
  </div>
  <div class="space-y-1">
    <Label for="jira-sync-interval">Sync interval (seconds)</Label>
    <Input
      id="jira-sync-interval"
      type="number"
      value={String(settings.sync_interval_ms / 1000)}
      onchange={(event) => {
        settings = { ...settings, sync_interval_ms: (parseInt(event.currentTarget.value, 10) || 60) * 1000 };
        save();
      }}
    />
    <p class="text-xs text-t3">Saved for a future sync worker; no periodic sync runs in this release.</p>
  </div>
</section>
