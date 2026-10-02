<script lang="ts">
  import type { Snippet } from "svelte";
  import { settingsWindow } from "./settings-window.svelte";

  /** Deep-link and search target for a registry setting; rings briefly when jumped to. */
  let { id, class: className = "", children }: { id: string; class?: string; children: Snippet } = $props();
</script>

<div
  id="setting-{id}"
  data-setting-id={id}
  class="transition-shadow duration-700 {settingsWindow.flashId === id ? 'ring-2 ring-inset ring-status-review/60' : ''} {className}"
>
  {@render children()}
</div>
