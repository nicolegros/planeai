<script lang="ts">
  import { getSettings } from "../../lib/settings.svelte";
  import { resolveSidebarGroupBy } from "../../lib/sidebar-model";
  import { SegmentedControl, Switch } from "../ui";
  import SettingRow from "./SettingRow.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import ModePicker from "./ModePicker.svelte";
  import ThemePicker from "./ThemePicker.svelte";
  import { saveSettings } from "../../lib/save-settings";

  const config = $derived(getSettings());
  const groupBy = $derived(resolveSidebarGroupBy(config));
</script>

<SettingsSection title="Color" help="Mode follows macOS when set to System.">
  <SettingRow id="appearance-mode">
    <ModePicker name="appearance-mode" />
  </SettingRow>
  <SettingRow id="theme" stacked>
    <ThemePicker />
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
