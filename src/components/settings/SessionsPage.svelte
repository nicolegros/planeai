<script lang="ts">
  import { CircleAlert } from "@lucide/svelte";
  import { preferences } from "../../lib/api";
  import { getSettings, updateSettings } from "../../lib/settings.svelte";
  import { Button, Dialog, SegmentedControl } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";

  type Backend = "local" | "tmux" | "daemon" | "rmux";

  const BACKENDS: readonly { value: Backend; label: string }[] = [
    { value: "local", label: "Local" },
    { value: "tmux", label: "tmux" },
    { value: "daemon", label: "Daemon (experimental)" },
    { value: "rmux", label: "rmux (experimental)" },
  ];

  const DESCRIPTIONS: Record<Backend, string> = {
    local: "Sessions run in-process and restart with the resume command on focus.",
    tmux: "Sessions persist after quitting (requires tmux).",
    daemon: "Sessions persist after quitting (built-in daemon, experimental).",
    rmux: "Sessions persist after quitting (requires rmux, experimental).",
  };

  const config = $derived(getSettings());
  const backend = $derived((BACKENDS.some((option) => option.value === config.session_backend) ? config.session_backend : "local") as Backend);
  let tmuxAvailable = $state(true);
  let rmuxAvailable = $state(true);
  const missingBinary = $derived((backend === "tmux" && !tmuxAvailable) || (backend === "rmux" && !rmuxAvailable));

  let staleWorktrees = $state<{ session_name: string; worktree_path: string; branch: string }[]>([]);
  let showCleanupDialog = $state(false);
  let cleanupMessage = $state("");

  $effect(() => {
    preferences.checkTmuxAvailable().then((available) => (tmuxAvailable = available));
    // A failed check must not block the page; assume available and show no warning.
    preferences.checkRmuxAvailable().then((available) => (rmuxAvailable = available)).catch(() => {});
  });

  async function previewCleanup() {
    cleanupMessage = "";
    try {
      const items = await preferences.listStaleWorktrees();
      if (items.length === 0) {
        cleanupMessage = "No stale worktrees found.";
      } else {
        staleWorktrees = items;
        showCleanupDialog = true;
      }
    } catch (error) {
      cleanupMessage = `Failed to list worktrees: ${error}`;
    }
  }

  async function confirmCleanup() {
    try {
      const errors = await preferences.runStaleWorktreeCleanup();
      cleanupMessage = errors.length ? `Cleanup finished with ${errors.length} error(s).` : "Cleanup complete.";
    } catch (error) {
      cleanupMessage = `Cleanup failed: ${error}`;
    } finally {
      showCleanupDialog = false;
      staleWorktrees = [];
    }
  }
</script>

<SettingsSection title="Backend" help="Changes apply to new sessions only; existing sessions keep their backend.">
  <SettingRow id="session-backend" stacked>
    {#snippet note()}
      <p class="mt-0.5 text-[12px] leading-snug text-t3">{DESCRIPTIONS[backend]}</p>
      {#if missingBinary}
        <p class="mt-1 flex items-center gap-1.5 text-[12px] text-status-review" role="alert">
          <CircleAlert size={12} />{backend} not found on PATH. Sessions will fail to launch.
        </p>
      {/if}
    {/snippet}
    <SegmentedControl
      name="session-backend"
      label="Run sessions in"
      options={BACKENDS}
      value={backend}
      onValueChange={(value) => updateSettings({ session_backend: value === "local" ? null : value })}
    />
  </SettingRow>
</SettingsSection>

<SettingsSection title="Maintenance">
  <SettingRow id="stale-worktrees">
    {#snippet note()}
      {#if cleanupMessage}<p class="mt-1 text-[12px] text-t2" role="status">{cleanupMessage}</p>{/if}
    {/snippet}
    <Button type="button" onclick={previewCleanup}>Clean up…</Button>
  </SettingRow>
</SettingsSection>

<Dialog open={showCleanupDialog} onOpenChange={(open) => (showCleanupDialog = open)} title="Clean up worktrees" class="w-[30rem] space-y-4 p-5">
  <h2 class="text-[15px] font-semibold text-t1">Remove these worktrees?</h2>
  <ul class="max-h-60 space-y-2 overflow-y-auto">
    {#each staleWorktrees as worktree (worktree.worktree_path)}
      <li class="rounded-md border border-border p-2 text-[12px]">
        <p class="font-medium text-t1">{worktree.session_name || "(unnamed session)"}</p>
        <p class="font-mono text-t3">{worktree.worktree_path}</p>
        {#if worktree.branch}<p class="text-t3">branch: {worktree.branch}</p>{/if}
      </li>
    {/each}
  </ul>
  <p class="flex items-center gap-1.5 text-[12px] text-status-review"><CircleAlert size={12} />The worktree folders are deleted. This cannot be undone.</p>
  <div class="flex justify-end gap-2 pt-1">
    <Button type="button" onclick={() => (showCleanupDialog = false)}>Cancel</Button>
    <Button type="button" variant="danger" onclick={confirmCleanup}>Remove {staleWorktrees.length} worktree{staleWorktrees.length === 1 ? "" : "s"}</Button>
  </div>
</Dialog>
