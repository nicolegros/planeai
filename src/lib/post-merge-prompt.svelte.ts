/**
 * Post-merge prompt store — shows Archive/Destroy/Keep prompt when a PR is merged.
 * When the session has a task, shows Done/Dismiss instead.
 * Default timeout action is configurable via settings.post_merge_action.
 */
import { showSnackbar } from "./snackbar.svelte";
import { getSettings } from "./settings.svelte";

export interface MergePrompt {
  sessionId: string;
  sessionName: string;
  taskKey: string | null;
  onArchive: (id: string) => Promise<void>;
  onDestroy: (id: string) => Promise<void>;
  onTaskDone: ((id: string) => Promise<void>) | null;
}

export interface MergePromptOptions {
  sessionId: string;
  sessionName: string;
  taskKey: string | null;
  onArchive: (id: string) => Promise<void>;
  onDestroy: (id: string) => Promise<void>;
  onTaskDone?: (id: string) => Promise<void>;
}

let prompt = $state<MergePrompt | null>(null);
let timer: ReturnType<typeof setTimeout> | null = null;

export function getPrompt(): MergePrompt | null {
  return prompt;
}

export function showMergePrompt(options: MergePromptOptions): void {
  clearTimer();
  prompt = {
    sessionId: options.sessionId,
    sessionName: options.sessionName,
    taskKey: options.taskKey,
    onArchive: options.onArchive,
    onDestroy: options.onDestroy,
    onTaskDone: options.onTaskDone ?? null,
  };

  const action = getSettings().post_merge_action ?? "archive";
  if (action === "keep") return; // no timeout
  timer = setTimeout(() => runDefault(), 30_000);
}

export async function handleArchive(): Promise<void> {
  if (!prompt) return;
  const { sessionId, onArchive } = prompt;
  clearTimer();
  prompt = null;
  await onArchive(sessionId);
}

export async function handleDestroy(): Promise<void> {
  if (!prompt) return;
  const { sessionId, onDestroy } = prompt;
  clearTimer();
  prompt = null;
  await onDestroy(sessionId);
}

export async function handleTaskDone(): Promise<void> {
  if (!prompt?.onTaskDone) return;
  const { sessionId, onTaskDone } = prompt;
  clearTimer();
  prompt = null;
  await onTaskDone(sessionId);
}

export function handleKeep(): void {
  clearTimer();
  prompt = null;
}

function runDefault(): void {
  if (!prompt) return;
  const action = getSettings().post_merge_action ?? "archive";
  const { sessionId, taskKey, onArchive, onDestroy, onTaskDone } = prompt;
  const hasTask = !!taskKey;
  prompt = null;
  timer = null;

  const taskThen =
    hasTask && onTaskDone ? onTaskDone(sessionId).catch(() => {}) : Promise.resolve();

  if (action === "archive") {
    taskThen
      .then(() => onArchive(sessionId))
      .then(() =>
        showSnackbar(hasTask ? "Task done, session archived" : "Session auto-archived", "success"),
      )
      .catch((e) => showSnackbar(String(e), "error"));
  } else if (action === "destroy") {
    taskThen
      .then(() => onDestroy(sessionId))
      .then(() =>
        showSnackbar(
          hasTask ? "Task done, session destroyed" : "Session auto-destroyed",
          "success",
        ),
      )
      .catch((e) => showSnackbar(String(e), "error"));
  } else if (hasTask && onTaskDone) {
    taskThen
      .then(() => showSnackbar("Task marked done", "success"))
      .catch((e) => showSnackbar(String(e), "error"));
  }
}

function clearTimer(): void {
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }
}

/** Dismiss prompt for a session that was removed externally. */
export function dismissForSession(sessionId: string): void {
  if (prompt?.sessionId === sessionId) {
    clearTimer();
    prompt = null;
  }
}

/** Focus coordination — component registers its focus function */
let focusFn: (() => void) | null = null;
export function registerFocus(fn: () => void) {
  focusFn = fn;
}
export function unregisterFocus() {
  focusFn = null;
}
export function focusMergePrompt() {
  focusFn?.();
}
