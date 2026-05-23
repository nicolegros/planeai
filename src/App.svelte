<script lang="ts">
  import { onMount } from "svelte";
  import { getActiveZone } from "./lib/focus.svelte";
  import { installKeyboardRouter } from "./lib/keyboard";

  let cleanup: (() => void) | undefined;

  onMount(() => {
    cleanup = installKeyboardRouter((action) => {
      // Future: dispatch to session store, tab switcher, etc.
      console.debug("[keyboard]", action.type);
    });
    return () => cleanup?.();
  });

  const zone = $derived(getActiveZone());
</script>

<main class="flex h-screen">
  <aside
    class="w-56 border-r border-neutral-800 p-3 {zone === 'sidebar' ? 'bg-neutral-900' : 'bg-neutral-950'}"
  >
    <h2 class="text-sm font-semibold text-neutral-400 mb-2">Sessions</h2>
    <p class="text-xs text-neutral-500">No sessions yet. Press Cmd+N.</p>
  </aside>

  <section class="flex-1 flex items-center justify-center">
    <p class="text-neutral-500">Terminal area — zone: {zone}</p>
  </section>
</main>
