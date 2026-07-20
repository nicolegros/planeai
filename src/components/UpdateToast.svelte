<script lang="ts">
  import { getUpdateState, dismissUpdate, setInstalling, registerUpdateFocus, unregisterUpdateFocus } from "../lib/updater.svelte";
  import { updater } from "../lib/api";
  import { refocusTerminal } from "../lib/focus.svelte";

  const updateState = $derived(getUpdateState());
  let wrapperEl = $state<HTMLDivElement | null>(null);
  let focused = $state(false);

  $effect(() => {
    registerUpdateFocus(() => { wrapperEl?.focus(); focused = true; });
    return unregisterUpdateFocus;
  });
  $effect(() => { if (!updateState.updateAvailable || updateState.dismissed) focused = false; });

  async function handleInstall() {
    if (updateState.installing) return;
    setInstalling(true);
    try {
      await updater.install();
    } catch (e) {
      console.error("Failed to install update:", e);
      setInstalling(false);
    }
  }

  function dismiss() {
    dismissUpdate();
    focused = false;
    refocusTerminal();
  }

  function onKeydown(e: KeyboardEvent) {
    if (updateState.installing) return;
    if (e.key === "i" || e.key === "I") { e.preventDefault(); handleInstall(); }
    else if (e.key === "d" || e.key === "D" || e.key === "Escape") { e.preventDefault(); dismiss(); }
  }
</script>

{#if updateState.updateAvailable && !updateState.dismissed}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={wrapperEl}
    role="alertdialog"
    aria-label="Application update available"
    tabindex="-1"
    onkeydown={onKeydown}
    class="fixed bottom-4 right-4 z-[100] flex items-center gap-3 rounded-lg bg-panel px-4 py-3 shadow-lg border border-border outline-none"
  >
    <div class="flex flex-col gap-0.5">
      <p class="text-sm font-medium text-t1">
        Update available: v{updateState.updateAvailable.version}
      </p>
      {#if updateState.updateAvailable.body}
        <p class="text-xs text-t3 max-w-[200px] truncate">{updateState.updateAvailable.body}</p>
      {/if}
    </div>
    <div class="flex items-center gap-2">
      {#if updateState.installing}
        <span class="text-xs text-t3">Installing...</span>
      {:else}
        <button
          class="rounded px-3 py-1.5 text-xs font-medium bg-accent text-on-accent hover:opacity-90 transition-opacity"
          onclick={handleInstall}
        >
          <span class="underline">I</span>nstall &amp; Restart
        </button>
        <button
          class="rounded px-2 py-1.5 text-xs text-t3 hover:text-t1 transition-colors"
          onclick={dismiss}
        >
          <span class="underline">D</span>ismiss
        </button>
      {/if}
    </div>
    <p class="text-[10px] text-t3">{focused ? "I / D / Esc" : "⌘U to interact"}</p>
  </div>
{/if}
