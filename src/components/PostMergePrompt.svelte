<script lang="ts">
  import { getPrompt, handleArchive, handleDestroy, handleKeep } from "../lib/post-merge-prompt.svelte";

  const prompt = $derived(getPrompt());
</script>

{#if prompt}
  <div class="fixed bottom-4 left-4 z-[100] max-w-lg rounded-lg bg-emerald-700 px-4 py-3 shadow-lg">
    <p class="text-sm text-white font-mono">PR merged ✓ — <span class="text-emerald-200">{prompt.sessionName}</span></p>
    <div class="mt-2 flex gap-2">
      <button
        class="rounded bg-white/20 px-3 py-1 text-xs font-medium text-white hover:bg-white/30"
        onclick={() => handleArchive()}
      >Archive</button>
      <button
        class="rounded bg-white/20 px-3 py-1 text-xs font-medium text-white hover:bg-white/30"
        onclick={() => handleDestroy()}
      >Destroy</button>
      <button
        class="rounded bg-white/10 px-3 py-1 text-xs font-medium text-white/80 hover:bg-white/20"
        onclick={() => handleKeep()}
      >Keep</button>
    </div>
    <p class="mt-1 text-xs text-emerald-200/70">Auto-archives in 30s</p>
  </div>
{/if}
