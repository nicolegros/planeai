<script lang="ts">
  import type { StagedPluginInstall } from "../lib/types";
  import { Button, Dialog } from "./ui";

  let { staged, busy, onConfirm, onCancel }: {
    staged: StagedPluginInstall | null;
    busy: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();

  const replacementNotice = $derived.by(() => {
    if (!staged) return null;
    const { replaces } = staged;
    if (replaces.kind === "repo") return `Updates v${replaces.installed_version} to v${staged.manifest.version}.`;
    if (replaces.kind === "local") {
      return replaces.original_display_path
        ? `Replaces your local build at ${replaces.original_display_path}. Data and settings are kept.`
        : "Replaces your local build. Data and settings are kept.";
    }
    return null;
  });
</script>

<Dialog
  open={staged !== null}
  onOpenChange={(open) => { if (!open && !busy) onCancel(); }}
  title="Install plugin"
  class="w-[520px] p-6 space-y-4"
>
  {#if staged}
    <h2 class="text-sm font-semibold text-t1">Install {staged.manifest.name}?</h2>
    {#if replacementNotice}
      <p class="text-sm text-t2">{replacementNotice}</p>
    {/if}
    <dl class="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1 font-mono text-xs text-t3">
      <dt>id</dt>
      <dd class="text-t2">{staged.manifest.id}</dd>
      <dt>version</dt>
      <dd class="text-t2">{staged.manifest.version}</dd>
      <dt>repository</dt>
      <dd class="text-t2">github.com/{staged.repo}</dd>
      <dt>source</dt>
      <dd class="text-t2 break-all">{staged.source_url}</dd>
      <dt>release</dt>
      <dd class="text-t2">{staged.release_tag ?? "none"}</dd>
      <dt>sha-256</dt>
      <dd class="text-t2 break-all">
        {#if staged.integrity.kind === "verified"}
          <span class="mr-1.5 rounded-full bg-status-running/15 px-2 py-0.5 font-sans text-[11px] font-medium text-status-running">Verified</span>
        {:else}
          <span class="mr-1.5 rounded-full bg-status-exited/15 px-2 py-0.5 font-sans text-[11px] font-medium text-status-exited">Unverified</span>
        {/if}{staged.integrity.sha256}
      </dd>
      <dt>capabilities</dt>
      <dd class="text-t2">{staged.manifest.capabilities.length > 0 ? staged.manifest.capabilities.join(", ") : "none"}</dd>
      <dt>ui</dt>
      <dd class="text-t2">{staged.manifest.ui_contributions.length > 0 ? staged.manifest.ui_contributions.map((contribution) => `${contribution.label} (${contribution.placement})`).join(", ") : "none"}</dd>
      <dt>tools</dt>
      <dd class="text-t2">
        {#each staged.manifest.tools as tool (tool.name)}
          <p class="break-words">{tool.name}: {tool.description}</p>
        {:else}
          none
        {/each}
      </dd>
    </dl>
    {#if staged.version_mismatch}
      <p class="rounded border border-status-review/30 bg-status-review/10 p-3 text-xs text-t2">The release is tagged {staged.version_mismatch.release_tag} but the package manifest says v{staged.version_mismatch.manifest_version}.</p>
    {/if}
    <p class="rounded border border-status-review/30 bg-status-review/10 p-3 text-xs text-t2">This is unverified third-party native code. It runs with your user permissions and PlaneAI cannot sandbox it.</p>
    <div class="flex justify-end gap-2 pt-2">
      <Button type="button" disabled={busy} onclick={onCancel}>Cancel</Button>
      <Button type="button" variant="primary" disabled={busy} onclick={onConfirm}>{busy ? "Installing…" : "Install and enable"}</Button>
    </div>
  {/if}
</Dialog>
