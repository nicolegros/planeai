/**
 * The snooze picker: one preset menu for every entry point (task context menu, `z` in the sidebar,
 * Mod+Option+Z), and the Custom dialog it leads to.
 */
import { tick } from "svelte";
import type { MenuItem } from "../components/ui/ContextMenu.svelte";
import {
  focusEditor,
  focusExplorer,
  focusSidebar,
  getActiveZone,
  refocusTerminal,
  type FocusZone,
} from "./focus.svelte";
import { snoozeTask, wakeTask } from "./task-moves";
import { getLaneNow } from "./snooze-clock.svelte";
import { DONE_CANNOT_SNOOZE, isTaskSnoozed, snoozeOptions } from "./task-snooze";
import * as taskStore from "./task-store.svelte";
import { showToast } from "./toasts.svelte";
import type { Project, TaskItem } from "./types";

export interface SnoozeTarget {
  task: TaskItem;
  project: Project;
}

let menu = $state<{ target: SnoozeTarget; x: number; y: number } | null>(null);
let custom = $state<SnoozeTarget | null>(null);
/** Where focus was before the picker took it, given back once the menu and its Custom dialog close. */
let returnFocus: { element: HTMLElement | null; zone: FocusZone } | null = null;

function rememberFocus(): void {
  if (returnFocus) return;
  const active = document.activeElement;
  const element = active instanceof HTMLElement && active !== document.body ? active : null;
  returnFocus = { element, zone: getActiveZone() };
}

const REFOCUS_ZONE: Record<FocusZone, () => void> = {
  terminal: refocusTerminal,
  sidebar: focusSidebar,
  editor: focusEditor,
  explorer: focusExplorer,
  none: () => {},
};

/** Waits a frame after the closing dialog unmounts, so its own focus handling cannot override this. */
function giveFocusBack(): void {
  if (menu || custom || !returnFocus) return;
  const { element, zone } = returnFocus;
  returnFocus = null;
  void tick().then(() =>
    requestAnimationFrame(() => {
      if (element?.isConnected) element.focus();
      else REFOCUS_ZONE[zone]();
    }),
  );
}

export function getSnoozeMenu() {
  return menu;
}

export function getCustomSnooze(): SnoozeTarget | null {
  return custom;
}

export function closeSnoozeMenu(): void {
  menu = null;
  giveFocusBack();
}

export function closeCustomSnooze(): void {
  custom = null;
  giveFocusBack();
}

function openCustomSnooze(target: SnoozeTarget): void {
  rememberFocus();
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

/** Snoozes from the Custom dialog, on the task as the store has it now. */
export function snoozeCustom(target: SnoozeTarget, at: Date): Promise<boolean> {
  const { task, project } = current(target);
  return snoozeTask(task, project, at);
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
  if (isTaskSnoozed(target.task, getLaneNow()))
    return { label: "Wake", onSelect: () => void wakeTask(target.task, target.project) };
  return { label: "Snooze", children: snoozeMenuItems(target, false) };
}

/** The `data-snooze-anchor` of a task's sidebar row, which the menu opens under. */
export function snoozeAnchor(projectId: string, key: string): string {
  return `${projectId}:${key}`;
}

/** Where the menu opens for a task: under its sidebar row, or near the top of the window without one. */
function anchorOf(target: SnoozeTarget): { x: number; y: number } {
  const row = document.querySelector(
    `[data-snooze-anchor="${CSS.escape(snoozeAnchor(target.project.id, target.task.key))}"]`,
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
    showToast({ kind: "info", message: DONE_CANNOT_SNOOZE, autoDismissMs: 3000 });
    return;
  }
  if (isTaskSnoozed(target.task, getLaneNow())) {
    void wakeTask(target.task, target.project);
    return;
  }
  rememberFocus();
  menu = { target, ...anchorOf(target) };
}
