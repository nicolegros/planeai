/**
 * Post-merge prompt store — shows Archive/Destroy/Keep prompt when a PR is merged.
 * Default timeout action is configurable via settings.post_merge_action.
 */
import { showSnackbar } from "./snackbar.svelte";
import { getSettings } from "./settings.svelte";

export interface MergePrompt {
  sessionId: string;
  sessionName: string;
}

let prompt = $state<MergePrompt | null>(null);
let timer: ReturnType<typeof setTimeout> | null = null;
let onArchive: ((id: string) => Promise<void>) | null = null;
let onDestroy: ((id: string) => Promise<void>) | null = null;

export function getPrompt(): MergePrompt | null {
  return prompt;
}

export function showMergePrompt(
  sessionId: string,
  sessionName: string,
  archiveFn: (id: string) => Promise<void>,
  destroyFn: (id: string) => Promise<void>,
): void {
  clearTimer();
  prompt = { sessionId, sessionName };
  onArchive = archiveFn;
  onDestroy = destroyFn;

  const action = getSettings().post_merge_action ?? "archive";
  if (action === "keep") return; // no timeout
  timer = setTimeout(() => runDefault(), 30_000);
}

export async function handleArchive(): Promise<void> {
  if (!prompt || !onArchive) return;
  const id = prompt.sessionId;
  clearTimer();
  prompt = null;
  await onArchive(id);
}

export async function handleDestroy(): Promise<void> {
  if (!prompt || !onDestroy) return;
  const id = prompt.sessionId;
  clearTimer();
  prompt = null;
  await onDestroy(id);
}

export function handleKeep(): void {
  clearTimer();
  prompt = null;
}

function runDefault(): void {
  if (!prompt) return;
  const action = getSettings().post_merge_action ?? "archive";
  const id = prompt.sessionId;
  prompt = null;
  timer = null;

  if (action === "archive" && onArchive) {
    onArchive(id).then(() => showSnackbar("Session auto-archived", "success"));
  } else if (action === "destroy" && onDestroy) {
    onDestroy(id).then(() => showSnackbar("Session auto-destroyed", "success"));
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
