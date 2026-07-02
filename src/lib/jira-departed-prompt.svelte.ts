/**
 * Jira departed-issue prompt store — shows "Mark done?" toast when issues disappear from JQL.
 * Queues prompts one-at-a-time. Re-prompts on next sync cycle if dismissed.
 */
import { listen } from "@tauri-apps/api/event";
import { refocusTerminal } from "./focus.svelte";

export interface DepartedPrompt {
  key: string;
  summary: string;
}

interface DepartedEvent {
  key: string;
  summary: string;
}

let queue = $state<DepartedPrompt[]>([]);
let current = $state<DepartedPrompt | null>(null);
let unlistenFn: (() => void) | null = null;

export function getCurrent(): DepartedPrompt | null {
  return current;
}

function advance() {
  if (queue.length > 0) {
    current = queue.shift()!;
  } else {
    current = null;
  }
}

export async function handleDone(): Promise<void> {
  if (!current) return;
  const { key } = current;
  current = null;
  advance();
  refocusTerminal();

  // Mark the task done via API
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    await invoke("move_task_item", { key, status: "done" });
  } catch (e) {
    console.error("Failed to mark Jira task done:", e);
  }
}

export function handleDismiss(): void {
  current = null;
  advance();
  refocusTerminal();
}

/** Start listening for Tauri events. Call once at app startup. */
export async function startListening(): Promise<void> {
  if (unlistenFn) return;
  unlistenFn = await listen<DepartedEvent>("jira-issue-departed", (event) => {
    const { key, summary } = event.payload;
    // Deduplicate: don't queue if already current or in queue
    if (current?.key === key) return;
    if (queue.some((p) => p.key === key)) return;
    if (!current) {
      current = { key, summary };
    } else {
      queue.push({ key, summary });
    }
  });
}

/** Stop listening. Call on teardown if needed. */
export function stopListening(): void {
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
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
export function focusDepartedPrompt() {
  focusFn?.();
}
