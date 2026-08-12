<script lang="ts">
  import { onMount } from "svelte";
  import { emit } from "@tauri-apps/api/event";
  import { plugins } from "../lib/api";
  import type { PluginInventory } from "../lib/types";
  import { Button } from "./ui";

  let { onOpenWorkspace }: { onOpenWorkspace?: () => void | Promise<void> } = $props();

  let inventory = $state<PluginInventory[]>([]);
  let busyId = $state<string | null>(null);
  let loadError = $state<string | null>(null);

  async function refresh() {
    try {
      inventory = await plugins.list();
      loadError = null;
    } catch (error) {
      loadError = String(error);
    }
  }

  async function run(id: string, action: "enable" | "disable" | "reload") {
    busyId = id;
    try {
      if (action !== "enable") await emit("plugin-page-close", id);
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

  async function openWorkspace(plugin: PluginInventory) {
    await emit("plugin-page-open", plugin.id);
    await onOpenWorkspace?.();
  }

  onMount(() => { void refresh(); });
</script>

<section class="space-y-3">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-sm font-medium text-t3 uppercase tracking-wide">Plugins</h2>
      <p class="mt-1 text-xs text-t3">Bundled plugins run as trusted, supervised subprocesses.</p>
    </div>
    <Button type="button" onclick={() => void refresh()}>Refresh</Button>
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
      {#if plugin.last_error}
        <p class="text-xs text-status-exited break-words">{plugin.last_error}</p>
      {/if}
      {#if plugin.log_path}
        <p class="font-mono text-[11px] text-t3 break-all">Log: {plugin.log_path}</p>
      {/if}
      <div class="flex flex-wrap gap-2">
        {#if plugin.state === "disabled" || plugin.state === "error"}
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "enable")}>
            {busyId === plugin.id ? "Starting…" : "Enable"}
          </Button>
        {:else if plugin.state === "running"}
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void openWorkspace(plugin)}>Open page</Button>
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "reload")}>
            {busyId === plugin.id ? "Reloading…" : "Reload"}
          </Button>
          <Button type="button" disabled={busyId === plugin.id} onclick={() => void run(plugin.id, "disable")}>
            Disable
          </Button>
        {:else}
          <span class="text-xs text-t3">{plugin.state === "starting" ? "Starting plugin…" : "Stopping plugin…"}</span>
        {/if}
      </div>
    </article>
  {:else}
    <p class="text-sm text-t3">No bundled plugins were discovered.</p>
  {/each}
</section>
