/**
 * The snooze picker: one preset menu for every entry point (task context menu, `z` in the sidebar,
 * Mod+Option+Z), and the Custom dialog it leads to.
 */
import type { MenuItem } from "../components/ui/ContextMenu.svelte";
import { snoozeTask, wakeTask } from "./task-moves";
import { isTaskSnoozed, snoozeOptions } from "./task-snooze";
import * as taskStore from "./task-store.svelte";
import { showToast } from "./toasts.svelte";
import type { Project, TaskItem } from "./types";

export interface SnoozeTarget {
  task: TaskItem;
  project: Project;
}

let menu = $state<{ target: SnoozeTarget; x: number; y: number } | null>(null);
let custom = $state<SnoozeTarget | null>(null);

export function getSnoozeMenu() {
  return menu;
}

export function getCustomSnooze(): SnoozeTarget | null {
  return custom;
}

export function closeSnoozeMenu(): void {
  menu = null;
}

export function closeCustomSnooze(): void {
  custom = null;
}

export function openCustomSnooze(target: SnoozeTarget): void {
  menu = null;
  custom = target;
}

/** The task as the store has it now, since a menu may outlive the row it opened from. */
function current(target: SnoozeTarget): SnoozeTarget {
  return {
    ...target,
    task: taskStore.findTask(target.project.path, target.task.key) ?? target.task,
  };
}

/** Snoozes at the preset's time as of the click; a time that passed meanwhile is refused. */
function pick(target: SnoozeTarget, resolve: (now: Date) => Date): void {
  const { task, project } = current(target);
  void snoozeTask(task, project, resolve(new Date()));
}

/** Presets soonest first, then Custom; `numbered` gives them the keys 1-5 and C. */
export function snoozeMenuItems(target: SnoozeTarget, numbered: boolean): MenuItem[] {
  const presets: MenuItem[] = snoozeOptions(new Date()).map((option, index) => ({
    label: option.label,
    shortcut: numbered ? String(index + 1) : undefined,
    onSelect: () => pick(target, option.resolve),
  }));
  return [
    ...presets,
    { separator: true },
    {
      label: "Custom…",
      shortcut: numbered ? "C" : undefined,
      onSelect: () => openCustomSnooze(target),
    },
  ];
}

/** The task menu's entry: Wake on a snoozed task, a Snooze submenu otherwise; none on a Done task. */
export function snoozeMenuEntry(target: SnoozeTarget): MenuItem | null {
  if (target.task.status === "done") return null;
  if (isTaskSnoozed(target.task, Date.now()))
    return { label: "Wake", onSelect: () => void wakeTask(target.task, target.project) };
  return { label: "Snooze", children: snoozeMenuItems(target, false) };
}

/** Where the menu opens for a task: under its sidebar row, or near the top of the window without one. */
function anchorOf(target: SnoozeTarget): { x: number; y: number } {
  const row = document.querySelector(
    `[data-snooze-anchor="${CSS.escape(`${target.project.id}:${target.task.key}`)}"]`,
  );
  const rect = row?.getBoundingClientRect();
  if (rect && rect.width > 0) return { x: rect.left + 8, y: rect.bottom };
  return { x: Math.max(8, window.innerWidth / 2 - 100), y: Math.round(window.innerHeight * 0.12) };
}

/** `z` and Mod+Option+Z: opens the preset menu, or wakes a task that is snoozed. */
export function snoozeOrWake(target: SnoozeTarget | null): void {
  if (!target) {
    showToast({ kind: "info", message: "No task is open.", autoDismissMs: 3000 });
    return;
  }
  if (target.task.status === "done") {
    showToast({ kind: "info", message: "Done tasks cannot be snoozed.", autoDismissMs: 3000 });
    return;
  }
  if (isTaskSnoozed(target.task, Date.now())) {
    void wakeTask(target.task, target.project);
    return;
  }
  menu = { target, ...anchorOf(target) };
}
