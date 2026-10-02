<script lang="ts">
  import type { PluginInventory } from "../../lib/types";
  import PluginManager from "../PluginManager.svelte";

  let { onInventoryChange }: { onInventoryChange: (inventory: PluginInventory[]) => void } = $props();
</script>

<div id="setting-plugins" data-setting-id="plugins">
  <PluginManager {onInventoryChange} />
</div>
