<script lang="ts">
  import { getPrompt, getCountdown, handleArchive, handleDestroy, handleKeep, handleTaskDone, registerFocus, unregisterFocus } from "../lib/post-merge-prompt.svelte";
  import { getSettings } from "../lib/settings.svelte";
  import { refocusTerminal } from "../lib/focus.svelte";

  const prompt = $derived(getPrompt());
  const countdown = $derived(getCountdown());
  let wrapperEl = $state<HTMLDivElement | null>(null);
  let focused = $state(false);

  $effect(() => {
    registerFocus(() => { wrapperEl?.focus(); focused = true; });
    return unregisterFocus;
  });
  $effect(() => { if (!prompt) focused = false; });

  function act(fn: () => void | Promise<void>) {
    fn();
    focused = false;
    refocusTerminal();
  }

  function onKeydown(e: KeyboardEvent) {
    if (prompt?.taskKey) {
      if (e.key === "d" || e.key === "D") { e.preventDefault(); act(handleTaskDone); }
      else if (e.key === "n" || e.key === "N" || e.key === "Escape") { e.preventDefault(); act(handleKeep); }
    } else {
      if (e.key === "a" || e.key === "A") { e.preventDefault(); act(handleArchive); }
      else if (e.key === "d" || e.key === "D") { e.preventDefault(); act(handleDestroy); }
      else if (e.key === "k" || e.key === "K" || e.key === "Escape") { e.preventDefault(); act(handleKeep); }
    }
  }
</script>

{#if prompt}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div bind:this={wrapperEl} tabindex="-1" onkeydown={onKeydown} class="fixed bottom-4 left-4 z-[100] max-w-lg rounded-lg bg-emerald-700 px-4 py-3 shadow-lg outline-none">
    <p class="text-sm text-white font-mono">PR merged ✓ — <span class="text-emerald-200">{prompt.sessionName}</span></p>
    <div class="mt-2 flex gap-2">
      {#if prompt.taskKey}
        <button
          class="rounded bg-white/20 px-3 py-1 text-xs font-medium text-white hover:bg-white/30"
          onclick={() => act(handleTaskDone)}
        ><span class="underline">D</span>one</button>
        <button
          class="rounded bg-white/10 px-3 py-1 text-xs font-medium text-white/80 hover:bg-white/20"
          onclick={() => act(handleKeep)}
        ><span class="underline">N</span>othing</button>
      {:else}
        <button
          class="rounded bg-white/20 px-3 py-1 text-xs font-medium text-white hover:bg-white/30"
          onclick={() => act(handleArchive)}
        ><span class="underline">A</span>rchive</button>
        <button
          class="rounded bg-white/20 px-3 py-1 text-xs font-medium text-white hover:bg-white/30"
          onclick={() => act(handleDestroy)}
        ><span class="underline">D</span>estroy</button>
        <button
          class="rounded bg-white/10 px-3 py-1 text-xs font-medium text-white/80 hover:bg-white/20"
          onclick={() => act(handleKeep)}
        ><span class="underline">K</span>eep</button>
      {/if}
    </div>
    <p class="mt-1 text-xs text-emerald-200/70">{focused ? (prompt.taskKey ? "D / N / Esc" : "A / D / K / Esc") : "⌘U to interact"}{countdown > 0 ? ` · auto-${(getSettings().post_merge_action ?? "archive") === "destroy" ? "destroys" : "archives"} in ${countdown}s` : ""}</p>
  </div>
{/if}
