import type { MenuItem } from "../components/ui/ContextMenu.svelte";
import type { AppConfig } from "./settings.svelte";
import { resolveSidebarGroupBy, resolveSidebarLayout } from "./sidebar-model";

type SidebarViewSettings = Pick<
  AppConfig,
  "sidebar_layout" | "sidebar_group_by" | "hide_task_keys" | "hide_project_labels"
>;

/** Items of the sidebar view options menu, opened by right-clicking the sidebar. */
export function sidebarViewMenuItems(
  settings: SidebarViewSettings,
  update: (patch: Partial<SidebarViewSettings>) => void,
): MenuItem[] {
  const groupBy = resolveSidebarGroupBy(settings);
  const layout = resolveSidebarLayout(settings);
  // Classic grouped by project shows rows under their project header, never a project label.
  const projectLabelsApply = layout === "cards" || groupBy === "status";
  return [
    {
      label: "Group by project",
      checked: groupBy === "project",
      radio: true,
      onSelect: () => update({ sidebar_group_by: "project" }),
    },
    {
      label: "Group by status",
      checked: groupBy === "status",
      radio: true,
      onSelect: () => update({ sidebar_group_by: "status" }),
    },
    { separator: true },
    {
      label: "Classic layout",
      checked: layout === "classic",
      radio: true,
      onSelect: () => update({ sidebar_layout: "classic" }),
    },
    {
      label: "Cards layout",
      checked: layout === "cards",
      radio: true,
      onSelect: () => update({ sidebar_layout: "cards" }),
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
      disabled: !projectLabelsApply,
      title: projectLabelsApply
        ? undefined
        : "Only applies when grouped by status or in the Cards layout",
      onSelect: () => update({ hide_project_labels: !settings.hide_project_labels }),
    },
  ];
}
