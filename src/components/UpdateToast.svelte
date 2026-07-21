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
  <!-- bottom-16 to stack above snackbar at bottom-4 -->
  <div
    bind:this={wrapperEl}
    role="alertdialog"
    aria-label="Application update available"
    tabindex="-1"
    onkeydown={onKeydown}
    class="fixed bottom-16 left-4 z-[100] flex items-center gap-3 rounded-lg bg-blue-600 px-4 py-3 shadow-lg outline-none"
  >
    {#if updateState.installing}
      <p class="text-sm text-white font-mono">Installing v{updateState.updateAvailable.version}…</p>
    {:else}
      <p class="text-sm text-white font-mono">Update available: v{updateState.updateAvailable.version}</p>
      <span class="text-blue-200 text-sm">·</span>
      <button
        class="text-sm text-white/90 hover:text-white transition-colors"
        onclick={handleInstall}
      ><span class="underline">I</span>nstall &amp; Restart</button>
      <button
        class="text-sm text-white/60 hover:text-white/90 transition-colors"
        onclick={dismiss}
      ><span class="underline">D</span>ismiss</button>
      {#if focused}
        <span class="text-xs text-blue-200">I / D / Esc</span>
      {:else}
        <span class="text-xs text-blue-200">⌘U</span>
      {/if}
    {/if}
  </div>
{/if}
