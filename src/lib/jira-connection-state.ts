import { listen } from "@tauri-apps/api/event";

/**
 * Subscribe safely to backend Jira connection changes, including unmount-before-registration.
 * `onRegistered` runs only after the event listener is active, so it is safe to use for an
 * initial status read without a gap where a disconnect notification could be missed.
 */
export function subscribeToJiraConnectionState(
  onConnectionStateChanged: () => void,
  onRegistered?: () => void,
): () => void {
  let disposed = false;
  let unlisten: (() => void) | undefined;

  void listen("jira-connection-state-changed", onConnectionStateChanged).then(
    (stop) => {
      unlisten = stop;
      if (disposed) stop();
      else onRegistered?.();
    },
    () => {
      // Preserve initial status loading even when event subscription is unavailable.
      if (!disposed) onRegistered?.();
    },
  );

  return () => {
    disposed = true;
    unlisten?.();
  };
}
