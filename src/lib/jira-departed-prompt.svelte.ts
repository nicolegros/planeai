/**
 * Jira departed-issue prompt store — shows "Mark done?" toast when issues disappear from JQL.
 * Queues prompts one-at-a-time. Re-prompts on next sync cycle if dismissed.
 */
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { createPromptQueue } from "./prompt-queue.svelte";

export interface DepartedPrompt {
  key: string;
  summary: string;
}

const queue = createPromptQueue<DepartedPrompt>((a, b) => a.key === b.key);

export const getCurrent = queue.getCurrent.bind(queue);
export const registerFocus = queue.registerFocus.bind(queue);
export const unregisterFocus = queue.unregisterFocus.bind(queue);
export const focusDepartedPrompt = queue.focus.bind(queue);

export async function handleDone(): Promise<void> {
  const prompt = queue.getCurrent();
  if (!prompt) return;
  const { key } = prompt;
  queue.dismiss();

  try {
    await invoke("move_task_item", { key, status: "done" });
  } catch (e) {
    console.error("Failed to mark Jira task done:", e);
  }
}

export function handleDismiss(): void {
  queue.dismiss();
}

let unlistenFn: (() => void) | null = null;

/** Start listening for Tauri events. Call once at app startup. */
export async function startListening(): Promise<void> {
  if (unlistenFn) return;
  unlistenFn = await listen<DepartedPrompt>("jira-issue-departed", (event) => {
    queue.push(event.payload);
  });
}

/** Stop listening. Call on teardown if needed. */
export function stopListening(): void {
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
  }
}
