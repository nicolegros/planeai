<script lang="ts">
  import { saveSettings } from "../../lib/save-settings";
  import { getSettings, type AppearanceMode } from "../../lib/settings.svelte";
  import { SegmentedControl } from "../ui";

  /** Radio group name; must differ when two pickers share a page. */
  let { name }: { name: string } = $props();

  const config = $derived(getSettings());
</script>

<SegmentedControl
  {name}
  label="Mode"
  options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]}
  value={config.appearance.mode}
  onValueChange={(mode: AppearanceMode) => saveSettings({ appearance: { ...config.appearance, mode } })}
/>
