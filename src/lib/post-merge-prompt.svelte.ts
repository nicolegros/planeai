/**
 * Post-merge prompt store — shows Archive/Destroy/Keep prompt when a PR is merged.
 * Auto-archives after 30s if no user interaction.
 */
import { showSnackbar } from "./snackbar.svelte";

export interface MergePrompt {
  sessionId: string;
  sessionName: string;
}

let prompt = $state<MergePrompt | null>(null);
let timer: ReturnType<typeof setTimeout> | null = null;
let onArchive: ((sessionId: string) => Promise<void>) | null = null;

export function getPrompt(): MergePrompt | null {
  return prompt;
}

export function showMergePrompt(
  sessionId: string,
  sessionName: string,
  archiveFn: (id: string) => Promise<void>,
): void {
  clearTimer();
  prompt = { sessionId, sessionName };
  onArchive = archiveFn;
  timer = setTimeout(() => {
    autoArchive();
  }, 30_000);
}

export async function handleArchive(): Promise<void> {
  if (!prompt || !onArchive) return;
  const id = prompt.sessionId;
  clearTimer();
  prompt = null;
  await onArchive(id);
}

export async function handleDestroy(destroyFn: (id: string) => Promise<void>): Promise<void> {
  if (!prompt) return;
  const id = prompt.sessionId;
  clearTimer();
  prompt = null;
  await destroyFn(id);
}

export function handleKeep(): void {
  clearTimer();
  prompt = null;
}

function autoArchive(): void {
  if (!prompt || !onArchive) return;
  const id = prompt.sessionId;
  prompt = null;
  timer = null;
  onArchive(id).then(() => {
    showSnackbar("Session auto-archived", "success");
  });
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
