<script lang="ts">
  let count = $state(0);
</script>

<main class="flex items-center justify-center h-screen">
  <div class="text-center">
    <h1 class="text-2xl font-bold mb-4">planeai</h1>
    <p class="text-neutral-400">Svelte 5 + Tauri v2</p>
    <button
      class="mt-4 px-4 py-2 bg-neutral-800 rounded hover:bg-neutral-700"
      onclick={() => count++}
    >
      count: {count}
    </button>
  </div>
</main>
