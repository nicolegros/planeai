import { renderTemplate } from "./render-template";
import type { TaskManagerTemplates } from "./settings.svelte";

/** The task fields the session templates can reference. */
export interface TemplateTask {
  key: string;
  title: string;
  description: string;
  priority: number;
  blocked_by: string[];
  tags: string[];
  parent_key: string | null;
  base_branch: string;
  status: string;
}

export interface TaskSessionDefaults {
  branch: string;
  name: string;
  prompt: string;
}

/**
 * The branch, name and prompt the backend's `plan_task_session` gives the task's session.
 * Both sides are checked against `src-tauri/src/task_start_cases.json`.
 */
export function taskSessionDefaults(
  task: TemplateTask,
  templates?: TaskManagerTemplates | null,
): TaskSessionDefaults {
  const render = (template?: string | null) =>
    template ? renderTemplate(template, task) : undefined;
  const firstLine = `Implement task ${task.key}: ${task.title}`;
  return {
    branch: render(templates?.branch) ?? `${task.key.toLowerCase()}/${titleSlug(task.title)}`,
    name: render(templates?.name) ?? `${task.key}: ${task.title}`,
    prompt:
      render(templates?.prompt) ??
      (task.description ? `${firstLine}\n\n${task.description}` : firstLine),
  };
}

function titleSlug(title: string): string {
  return title
    .toLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^a-z0-9\-/]/g, "")
    .replace(/-+$/, "");
}
