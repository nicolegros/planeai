/**
 * Post-merge prompt: a toast offering Archive/Destroy (or Done when the session has a task)
 * once a PR merges. Unless settings.post_merge_action is "keep", it runs that action after 30s.
 */
import { getSettings } from "./settings.svelte";
import { dismissToast, showToast, type ToastAction } from "./toasts.svelte";

export interface MergePromptOptions {
  sessionId: string;
  sessionName: string;
  taskKey: string | null;
  /** A generic completion heading, rendered by the host. */
  message?: string;
  onArchive: (id: string) => Promise<void>;
  onDestroy: (id: string) => Promise<void>;
  onTaskDone?: (id: string) => Promise<void>;
}

const TIMEOUT_SECONDS = 30;

function toastId(sessionId: string): string {
  return `post-merge:${sessionId}`;
}

function defaultAction() {
  return getSettings().post_merge_action ?? "archive";
}

export function showMergePrompt(options: MergePromptOptions): void {
  const { sessionId, taskKey, onArchive, onDestroy, onTaskDone } = options;
  const actions: ToastAction[] = taskKey
    ? [{ key: "Enter", label: "Done", run: () => onTaskDone?.(sessionId) }]
    : [
        { key: "a", label: "Archive", run: () => onArchive(sessionId) },
        { key: "x", label: "Destroy", run: () => onDestroy(sessionId) },
      ];

  showToast({
    id: toastId(sessionId),
    kind: "success",
    message: `${options.message?.trim() || "PR merged"} ✓`,
    detail: options.sessionName,
    actions,
    dismissLabel: "Keep",
    countdown:
      defaultAction() === "keep"
        ? undefined
        : {
            seconds: TIMEOUT_SECONDS,
            label: (remaining) =>
              `auto-${defaultAction() === "destroy" ? "destroys" : "archives"} in ${remaining}s`,
            onExpire: () => runDefault(options),
          },
  });
}

/** Dismiss the prompt of a session that was removed externally. */
export function dismissForSession(sessionId: string): void {
  dismissToast(toastId(sessionId));
}

function runDefault({
  sessionId,
  taskKey,
  onArchive,
  onDestroy,
  onTaskDone,
}: MergePromptOptions): void {
  const action = defaultAction();
  if (taskKey && onTaskDone) {
    // Moving a task to done makes the backend archive the session, so we must NOT
    // call onArchive/onDestroy afterwards (the session is already gone).
    onTaskDone(sessionId)
      .then(() =>
        showToast(
          action === "destroy" ? "Task done, session destroyed" : "Task done, session archived",
          "success",
        ),
      )
      .catch((e) => showToast(String(e), "error"));
  } else if (action === "archive") {
    onArchive(sessionId)
      .then(() => showToast("Session auto-archived", "success"))
      .catch((e) => showToast(String(e), "error"));
  } else if (action === "destroy") {
    onDestroy(sessionId)
      .then(() => showToast("Session auto-destroyed", "success"))
      .catch((e) => showToast(String(e), "error"));
  }
}
