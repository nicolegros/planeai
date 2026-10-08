import type { Project } from "./types";

interface ProjectContextMenuActions {
  onEdit: (project: Project) => void;
  onToggleAutoDispatch: (project: Project) => void;
  onHide: (id: string) => void;
  onArchive: (id: string) => void;
  onDelete: (project: Project) => void;
}

export function projectContextMenuItems(
  project: Project,
  autoDispatchEnabled: boolean,
  actions: ProjectContextMenuActions,
) {
  return [
    { label: "Edit project", onSelect: () => actions.onEdit(project) },
    {
      label: autoDispatchEnabled ? "✓ Auto-dispatch" : "Auto-dispatch",
      onSelect: () => actions.onToggleAutoDispatch(project),
    },
    { label: "Hide project", onSelect: () => actions.onHide(project.id) },
    { label: "Archive project", onSelect: () => actions.onArchive(project.id) },
    { label: "Delete project", danger: true, onSelect: () => actions.onDelete(project) },
  ];
}
