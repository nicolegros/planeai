<script lang="ts">
  import { onMount } from "svelte";
  import { plugins } from "../lib/api";
  import { showSnackbar } from "../lib/snackbar.svelte";
  import { Button, Input, Label } from "./ui";

  interface JiraSettings { site: string; sync_interval_ms: number; }
  interface JiraStatus { connected: boolean; authorizing: boolean; site: string | null; last_error: string | null; }

  let settings = $state<JiraSettings>({ site: "", sync_interval_ms: 60000 });
  let status = $state<JiraStatus>({ connected: false, authorizing: false, site: null, last_error: null });
  let loading = $state(true);
  let connecting = $state(false);
  let cancelling = $state(false);
  let connectionAttempt = 0;
  let activeAttemptId: string | null = null;
  let saveChain = Promise.resolve();
  let latestSave = 0;
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
      const jira = (await plugins.list()).find((plugin) => plugin.id === "jira");
      if (!jira) throw new Error("The bundled Jira plugin is unavailable.");
      if (jira.state !== "running") await plugins.enable("jira");
      await refresh();
    } catch (error) {
      showSnackbar(`Jira plugin could not start: ${String(error)}`, "error");
    } finally {
      loading = false;
    }
  });

  function save(): Promise<void> {
    const sequence = ++latestSave;
    const snapshot = { ...settings };
    const write = saveChain
      .catch(() => {})
      .then(() => plugins.updateSettings<JiraSettings>("jira", snapshot));
    saveChain = write.then(() => {}, () => {});
    return write.then((saved) => {
      if (sequence === latestSave) settings = saved;
    });
  }

  function saveAfterEdit(): void {
    void save().catch((error) => {
      showSnackbar(`Could not save Jira settings: ${String(error)}`, "error");
    });
  }

  async function connect(): Promise<void> {
    const attempt = ++connectionAttempt;
    const attemptId = crypto.randomUUID();
    activeAttemptId = attemptId;
    connecting = true;
    try {
      await save();
      if (attempt !== connectionAttempt) return;
      const { authorization_url } = await call<{ authorization_url: string }>("jira.connect.start", { attempt_id: attemptId });
      if (attempt !== connectionAttempt) {
        await call("jira.connect.cancel", { attempt_id: attemptId });
        return;
      }
      try {
        await plugins.openJiraAuthorizationUrl(authorization_url);
      } catch (error) {
        await call("jira.connect.cancel", { attempt_id: attemptId }).catch(() => {});
        throw new Error(`Could not open your browser. ${String(error)}`);
      }
      if (attempt !== connectionAttempt) return;
      showSnackbar("Finish Jira authorization in your browser…", "success");
      await call("jira.connect.complete", { attempt_id: attemptId });
      if (attempt !== connectionAttempt) {
        await call("jira.connect.cancel", { attempt_id: attemptId });
        return;
      }
      do {
        await new Promise((resolve) => setTimeout(resolve, 500));
        await refresh();
        if (attempt !== connectionAttempt) {
          await call("jira.connect.cancel", { attempt_id: attemptId });
          return;
        }
      } while (status.authorizing);
      if (!status.connected) throw new Error(status.last_error ?? "Jira authorization did not complete.");
      showSnackbar("Connected to Jira", "success");
    } catch (error) {
      if (attempt === connectionAttempt) {
        await refresh().catch(() => {});
        showSnackbar(`Jira connection failed: ${String(error)}`, "error");
      }
    } finally {
      if (attempt === connectionAttempt) {
        connecting = false;
        activeAttemptId = null;
      }
    }
  }

  async function cancelConnection(): Promise<void> {
    connectionAttempt += 1;
    const attemptId = activeAttemptId;
    cancelling = true;
    try {
      await call("jira.connect.cancel", attemptId ? { attempt_id: attemptId } : null);
      await refresh();
      showSnackbar("Jira authorization cancelled", "success");
    } catch (error) {
      showSnackbar(`Could not cancel Jira authorization: ${String(error)}`, "error");
    } finally {
      connecting = false;
      cancelling = false;
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
    {:else if connecting}
      <Button onclick={cancelConnection} disabled={cancelling}>{cancelling ? 'Cancelling…' : 'Cancel authorization'}</Button>
    {:else}
      <Button onclick={connect} disabled={!hasSite}>Connect</Button>
      {#if !hasSite}<span class="text-xs text-t3">Enter a site URL to connect</span>{/if}
    {/if}
  </div>

  <div class="space-y-1">
    <Label for="jira-site">Site URL</Label>
    <Input id="jira-site" bind:value={settings.site} placeholder="https://mycompany.atlassian.net" onchange={saveAfterEdit} disabled={loading || connecting || status.connected} />
  </div>
  <div class="space-y-1">
    <Label for="jira-sync-interval">Sync interval (seconds)</Label>
    <Input
      id="jira-sync-interval"
      type="number"
      value={String(settings.sync_interval_ms / 1000)}
      disabled={loading || connecting}
      onchange={(event) => {
        settings = { ...settings, sync_interval_ms: (parseInt(event.currentTarget.value, 10) || 60) * 1000 };
        saveAfterEdit();
      }}
    />
    <p class="text-xs text-t3">Saved for a future sync worker; no periodic sync runs in this release.</p>
  </div>
</section>
