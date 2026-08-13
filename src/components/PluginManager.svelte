<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { plugins } from "../lib/api";
  import type { PluginInventory } from "../lib/types";
  import { Button, Dialog } from "./ui";

  let inventory = $state<PluginInventory[]>([]);
  let busyId = $state<string | null>(null);
  let installing = $state(false);
  let loadError = $state<string | null>(null);
  let pendingRemoval = $state<PluginInventory | null>(null);

  async function refresh() {
    try {
      inventory = await plugins.list();
      loadError = null;
    } catch (error) {
      loadError = String(error);
    }
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


  onMount(() => { void refresh(); });
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
      {#if plugin.ui_contributions.length > 0}
        <p class="text-xs text-t3">Contributions: {plugin.ui_contributions.map((contribution) => `${contribution.label} (${contribution.placement})`).join(", ")}</p>
      {/if}
      <div class="flex flex-wrap gap-2">
        {#if plugin.state === "disabled" || plugin.state === "error"}
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "enable")}>
            {busyId === plugin.id ? "Starting…" : "Enable"}
          </Button>
        {:else if plugin.state === "running"}
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "reload")}>
            {busyId === plugin.id ? "Reloading…" : "Reload"}
          </Button>
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "disable")}>Disable</Button>
        {:else}
          <span class="text-xs text-t3">{plugin.state === "starting" ? "Starting plugin…" : "Stopping plugin…"}</span>
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
