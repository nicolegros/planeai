<script lang="ts">
  import { preferences } from "../../lib/api";
  import { getSettings, type AppearanceMode } from "../../lib/settings.svelte";
  import { resolveSidebarGroupBy } from "../../lib/sidebar-model";
  import { SegmentedControl, Switch } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import { saveSettings } from "../../lib/save-settings";

  const config = $derived(getSettings());
  const groupBy = $derived(resolveSidebarGroupBy(config));
  let availableThemes = $state<string[]>([]);
  const themes = $derived(availableThemes.includes(config.appearance.theme) ? availableThemes : [config.appearance.theme, ...availableThemes]);

  $effect(() => {
    preferences
      .listThemes()
      .then((names) => (availableThemes = names))
      .catch((error) => console.warn("Failed to list themes:", error));
  });
</script>

<SettingsSection title="Color" help="Mode follows macOS when set to System.">
  <SettingRow id="appearance-mode">
    <SegmentedControl
      name="appearance-mode"
      label="Mode"
      options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]}
      value={config.appearance.mode}
      onValueChange={(mode: AppearanceMode) => saveSettings({ appearance: { ...config.appearance, mode } })}
    />
  </SettingRow>
  <SettingRow id="theme" stacked>
    <div class="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Theme">
      {#each themes as theme (theme)}
        <button
          type="button"
          role="radio"
          aria-checked={config.appearance.theme === theme}
          class="rounded-md px-3 py-1 text-[12px] font-medium capitalize transition-colors {config.appearance.theme === theme ? 'bg-accent text-on-accent' : 'bg-panel-hi text-t2 hover:text-t1'}"
          onclick={() => saveSettings({ appearance: { ...config.appearance, theme } })}
        >{theme}</button>
      {/each}
    </div>
  </SettingRow>
</SettingsSection>

<SettingsSection title="Sidebar">
  <SettingRow id="sidebar-group-by">
    <SegmentedControl
      name="sidebar-group-by"
      label="Group tasks by"
      options={[{ value: "project", label: "Project" }, { value: "status", label: "Status" }]}
      value={groupBy}
      onValueChange={(value) => saveSettings({ sidebar_group_by: value })}
    />
  </SettingRow>
  <SettingRow id="show-task-keys">
    <Switch label="Show task keys" checked={!config.hide_task_keys} onCheckedChange={(checked) => saveSettings({ hide_task_keys: !checked })} />
  </SettingRow>
  <SettingRow id="show-project-labels" disabled={groupBy !== "status"}>
    <Switch
      label="Show project labels"
      checked={!config.hide_project_labels}
      disabled={groupBy !== "status"}
      onCheckedChange={(checked) => saveSettings({ hide_project_labels: !checked })}
    />
  </SettingRow>
  <SettingRow id="hide-empty-projects">
    <Switch label="Hide empty projects" checked={!!config.hide_empty_projects} onCheckedChange={(checked) => saveSettings({ hide_empty_projects: checked })} />
  </SettingRow>
</SettingsSection>
