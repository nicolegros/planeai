<script lang="ts">
  import { getCurrent, handleDone, handleDismiss, registerFocus, unregisterFocus } from "../lib/jira-departed-prompt.svelte";
  import { refocusTerminal } from "../lib/focus.svelte";
  import { MOD_LABEL } from "../lib/keyboard";

  const prompt = $derived(getCurrent());
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
    if (e.key === "d" || e.key === "D") { e.preventDefault(); act(handleDone); }
    else if (e.key === "n" || e.key === "N" || e.key === "Escape") { e.preventDefault(); act(handleDismiss); }
  }
</script>

{#if prompt}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div bind:this={wrapperEl} tabindex="-1" onkeydown={onKeydown} class="fixed bottom-4 left-4 z-[100] max-w-lg rounded-lg bg-amber-700 px-4 py-3 shadow-lg outline-none">
    <p class="text-sm text-white font-mono">Issue left JQL — <span class="text-amber-200">{prompt.key}</span></p>
    <p class="mt-0.5 text-xs text-amber-100/80 truncate">{prompt.summary}</p>
    <div class="mt-2 flex gap-2">
      <button
        class="rounded bg-white/20 px-3 py-1 text-xs font-medium text-white hover:bg-white/30"
        onclick={() => act(handleDone)}
      ><span class="underline">D</span>one</button>
      <button
        class="rounded bg-white/10 px-3 py-1 text-xs font-medium text-white/80 hover:bg-white/20"
        onclick={() => act(handleDismiss)}
      >Dismiss (<span class="underline">N</span>)</button>
    </div>
    <p class="mt-1 text-xs text-amber-200/70">{focused ? "D / N / Esc" : `${MOD_LABEL}U to interact`}</p>
  </div>
{/if}
