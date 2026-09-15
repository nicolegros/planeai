<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { plugins } from "../lib/api";
  import type { GithubMigrationStatus, JiraMigrationStatus, PluginInventory } from "../lib/types";
  import { Button, Dialog } from "./ui";

  let { onInventoryChange = (_inventory: PluginInventory[]) => {} }: {
    onInventoryChange?: (inventory: PluginInventory[]) => void;
  } = $props();
  let inventory = $state<PluginInventory[]>([]);
  let jiraMigration = $state<JiraMigrationStatus | null>(null);
  let githubMigration = $state<GithubMigrationStatus | null>(null);
  let busyId = $state<string | null>(null);
  let installing = $state(false);
  let loadError = $state<string | null>(null);
  let pendingRemoval = $state<PluginInventory | null>(null);

  async function refresh() {
    try {
      const [nextInventory, jiraStatus, githubStatus] = await Promise.all([
        plugins.list(),
        plugins.jiraMigrationStatus(),
        plugins.githubMigrationStatus(),
      ]);
      inventory = nextInventory;
      jiraMigration = jiraStatus;
      githubMigration = githubStatus;
      onInventoryChange(inventory);
      loadError = null;
    } catch (error) {
      loadError = String(error);
    }
  }

  function migrationBlocksEnable(pluginId: string): boolean {
    const migration = pluginId === "jira" ? jiraMigration : pluginId === "github" ? githubMigration : null;
    return migration !== null && migration.state !== "not_needed" && migration.state !== "completed";
  }

  async function installLocal() {
    const selected = await open({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    installing = true;
    try {
      await plugins.installLocal(selected);
      await refresh();
    } catch (error) {
      loadError = String(error);
      await refresh();
    } finally {
      installing = false;
    }
  }

  async function run(id: string, action: "enable" | "disable" | "reload") {
    busyId = id;
    try {
      if (action === "enable") await plugins.enable(id);
      else if (action === "disable") await plugins.disable(id);
      else await plugins.reload(id);
      await refresh();
    } catch (error) {
      loadError = String(error);
      await refresh();
    } finally {
      busyId = null;
    }
  }

  async function confirmRemoval() {
    const plugin = pendingRemoval;
    if (!plugin) return;
    busyId = plugin.id;
    try {
      await plugins.removeLocal(plugin.id);
      pendingRemoval = null;
      await refresh();
    } catch (error) {
      loadError = String(error);
      await refresh();
    } finally {
      busyId = null;
    }
  }

  async function migrateLegacyJira() {
    busyId = "jira";
    try {
      jiraMigration = await plugins.migrateLegacyJira();
      await refresh();
    } catch (error) {
      loadError = String(error);
      await refresh();
    } finally {
      busyId = null;
    }
  }

  async function migrateLegacyGithub() {
    busyId = "github";
    try {
      githubMigration = await plugins.migrateLegacyGithub();
      await refresh();
    } catch (error) {
      loadError = String(error);
      await refresh();
    } finally {
      busyId = null;
    }
  }

  onMount(() => {
    const unlistenGithubMigration = listen<GithubMigrationStatus>("github-migration-changed", (event) => {
      githubMigration = event.payload;
    });
    void refresh();
    return () => {
      void unlistenGithubMigration.then((unlisten) => unlisten());
    };
  });
</script>

<section class="space-y-3">
  <div class="flex items-center justify-between gap-4">
    <div>
      <h2 class="text-sm font-medium text-t3 uppercase tracking-wide">Plugins</h2>
      <p class="mt-1 text-xs text-t3">Trusted local packages run as supervised subprocesses from PlaneAI-owned imported copies.</p>
    </div>
    <div class="flex gap-2">
      <Button type="button" disabled={installing} onclick={() => void installLocal()}>{installing ? "Importing…" : "Install local package"}</Button>
      <Button type="button" onclick={() => void refresh()}>Refresh</Button>
    </div>
  </div>

  {#if loadError}
    <p class="rounded border border-status-exited/30 bg-status-exited/10 p-3 text-xs text-status-exited">{loadError}</p>
  {/if}

  {#if githubMigration && githubMigration.state !== "not_needed"}
    <section class="rounded border border-status-review/30 bg-status-review/10 p-3 space-y-2" aria-live="polite">
      <div>
        <h3 class="text-xs font-medium text-t1">Migrate existing GitHub pull request state</h3>
        <p class="mt-1 text-xs text-t2">{githubMigration.message}</p>
        <p class="mt-1 text-[11px] text-t3">Migration imports URL-backed legacy pull request mappings. State-only legacy values are safely skipped because the GitHub plugin requires a pull request URL.</p>
        <p class="mt-1 text-[11px] text-t3">Migration does not install or enable the local GitHub plugin. After it completes, install the local package and enable it normally.</p>
      </div>
      {#if githubMigration.error}
        <p class="text-xs text-status-exited break-words">{githubMigration.error}</p>
      {/if}
      {#if githubMigration.skipped_state_only > 0}
        <p class="text-xs text-t3">Skipped {githubMigration.skipped_state_only} state-only legacy {githubMigration.skipped_state_only === 1 ? "entry" : "entries"}.</p>
      {/if}
      {#if githubMigration.can_migrate}
        <Button type="button" disabled={busyId === "github"} onclick={() => void migrateLegacyGithub()}>
          {busyId === "github" ? "Migrating…" : githubMigration.state === "failed" ? "Retry GitHub migration" : "Migrate GitHub pull request state"}
        </Button>
      {:else if githubMigration.state === "importing"}
        <p class="text-xs text-t3">Migration is fenced until its current operation finishes or the app is restarted.</p>
      {:else if githubMigration.state === "completed"}
        <p class="text-xs text-t3">Migration is complete. Install the local GitHub plugin, then enable it normally.</p>
      {/if}
    </section>
  {/if}

  {#each inventory as plugin (plugin.id)}
    <article class="rounded-lg border border-border p-4 space-y-3">
      <div class="flex items-start justify-between gap-3">
        <div>
          <h3 class="text-sm font-medium text-t1">{plugin.name}</h3>
          <p class="mt-0.5 font-mono text-xs text-t3">{plugin.id} · {plugin.version} · {plugin.source_kind}</p>
        </div>
        <span class="rounded-full bg-panel-hi px-2 py-0.5 text-xs text-t2">{plugin.state}</span>
      </div>
      {#if plugin.source_kind === "local"}
        <div class="space-y-1 text-[11px] text-t3">
          {#if plugin.original_display_path}<p class="font-mono break-all">Source: {plugin.original_display_path}</p>{/if}
          {#if plugin.installed_hash}<p class="font-mono break-all">Installed SHA-256: {plugin.installed_hash}</p>{/if}
        </div>
      {/if}
      {#if plugin.last_error}
        <p class="text-xs text-status-exited break-words">{plugin.last_error}</p>
      {/if}
      {#if plugin.log_path}
        <p class="font-mono text-[11px] text-t3 break-all">Log: {plugin.log_path}</p>
      {/if}
      {#if plugin.id === "jira" && jiraMigration && jiraMigration.state !== "not_needed" && jiraMigration.state !== "completed"}
        <section class="rounded border border-status-review/30 bg-status-review/10 p-3 space-y-2" aria-live="polite">
          <div>
            <h4 class="text-xs font-medium text-t1">Migrate existing Jira state</h4>
            <p class="mt-1 text-xs text-t2">{jiraMigration.message}</p>
            <p class="mt-1 text-[11px] text-t3">Migration imports sites, sources, writeback settings, credentials, issues, links, and departed prompts. Legacy Jira remains disabled until the validated plugin import is enabled.</p>
          </div>
          {#if jiraMigration.error}
            <p class="text-xs text-status-exited break-words">{jiraMigration.error}</p>
          {/if}
          {#if jiraMigration.can_migrate}
            <Button type="button" disabled={busyId === "jira"} onclick={() => void migrateLegacyJira()}>
              {busyId === "jira" ? "Migrating…" : jiraMigration.state === "failed" ? "Retry migration" : "Migrate and enable Jira plugin"}
            </Button>
          {:else}
            <p class="text-xs text-t3">Migration is fenced until its current operation finishes or the app is restarted.</p>
          {/if}
        </section>
      {/if}
      {#if plugin.ui_contributions.length > 0}
        <p class="text-xs text-t3">Contributions: {plugin.ui_contributions.map((contribution) => `${contribution.label} (${contribution.placement})`).join(", ")}</p>
      {/if}
      <div class="flex flex-wrap gap-2">
        {#if (plugin.state === "disabled" || plugin.state === "error") && !migrationBlocksEnable(plugin.id)}
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "enable")}>
            {busyId === plugin.id ? "Starting…" : "Enable"}
          </Button>
        {:else if plugin.state === "running"}
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "reload")}>
            {busyId === plugin.id ? "Reloading…" : "Reload"}
          </Button>
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "disable")}>Disable</Button>
        {:else}
          <span class="text-xs text-t3">{migrationBlocksEnable(plugin.id) ? "Migration required before enabling this plugin." : plugin.state === "starting" ? "Starting plugin…" : "Stopping plugin…"}</span>
        {/if}
        {#if plugin.source_kind === "local"}
          <button
            type="button"
            class="rounded bg-status-exited/15 px-3 py-1.5 text-xs font-medium text-status-exited hover:bg-status-exited/25 disabled:opacity-50"
            disabled={busyId === plugin.id}
            onclick={() => pendingRemoval = plugin}
          >Remove</button>
        {/if}
      </div>
    </article>
  {:else}
    <p class="text-sm text-t3">No plugins were discovered.</p>
  {/each}
</section>

<Dialog
  open={pendingRemoval !== null}
  onOpenChange={(open) => { if (!open && busyId === null) pendingRemoval = null; }}
  title="Remove local plugin"
  class="w-[480px] p-6 space-y-4"
>
  {#if pendingRemoval}
    <h2 class="text-sm font-semibold text-t1">Remove {pendingRemoval.name}?</h2>
    <p class="text-sm text-t2">This deletes PlaneAI’s imported plugin bytes, settings, secrets, logs, and database data for <span class="font-mono">{pendingRemoval.id}</span>.</p>
    <p class="text-xs text-t3">The original package directory will not be modified or deleted.</p>
    <div class="flex justify-end gap-2 pt-2">
      <Button type="button" disabled={busyId !== null} onclick={() => pendingRemoval = null}>Cancel</Button>
      <button
        type="button"
        class="rounded bg-status-exited px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50"
        disabled={busyId !== null}
        onclick={() => void confirmRemoval()}
      >{busyId ? "Removing…" : "Remove plugin"}</button>
    </div>
  {/if}
</Dialog>
