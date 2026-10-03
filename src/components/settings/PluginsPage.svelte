<script lang="ts">
  import type { PluginInventory } from "../../lib/types";
  import PluginManager from "../PluginManager.svelte";
  import SettingAnchor from "./SettingAnchor.svelte";

  let { onInventoryChange }: { onInventoryChange: (inventory: PluginInventory[]) => void } = $props();
</script>

<SettingAnchor id="plugins">
  <PluginManager {onInventoryChange} />
</SettingAnchor>
