import type { MenuItem } from "../components/ui/ContextMenu.svelte";
import type { AppConfig } from "./settings.svelte";

type SidebarViewSettings = Pick<
  AppConfig,
  "sidebar_group_by" | "hide_task_keys" | "hide_project_labels"
>;

/** Items of the sidebar view options menu, opened by right-clicking the sidebar. */
export function sidebarViewMenuItems(
  settings: SidebarViewSettings,
  update: (patch: Partial<SidebarViewSettings>) => void,
): MenuItem[] {
  const groupBy = settings.sidebar_group_by ?? "project";
  return [
    {
      label: "Group by project",
      checked: groupBy === "project",
      onSelect: () => update({ sidebar_group_by: "project" }),
    },
    {
      label: "Group by status",
      checked: groupBy === "status",
      onSelect: () => update({ sidebar_group_by: "status" }),
    },
    { separator: true },
    {
      label: "Show task keys",
      checked: !settings.hide_task_keys,
      onSelect: () => update({ hide_task_keys: !settings.hide_task_keys }),
    },
    {
      label: "Show project labels",
      checked: !settings.hide_project_labels,
      disabled: groupBy === "project",
      title: groupBy === "project" ? "Only applies when grouped by status" : undefined,
      onSelect: () => update({ hide_project_labels: !settings.hide_project_labels }),
    },
  ];
}
