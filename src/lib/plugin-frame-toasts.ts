import { dismissToast, showToast, type ToastDismiss, type ToastKind } from "./toasts.svelte";

const TOAST_KINDS: readonly ToastKind[] = ["error", "warning", "info", "success"];

/**
 * A toast as a local plugin frame posts it (host API v7). Its callbacks stay in the frame:
 * actions are named by key, and `notify` asks the host to report a user dismissal back.
 */
export interface FrameToast {
  id: string;
  kind: ToastKind;
  message: string;
  detail?: string;
  actions: Array<{ key: string; label: string }>;
  dismiss: "enabled" | "disabled" | "notify";
}

/** What the host posts back to a frame when the user acts on one of its toasts. */
export type FrameToastEvent =
  | { type: "toast-event"; id: string; action: string }
  | { type: "toast-event"; id: string; dismiss: true };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim() !== "";
}

export function parseFrameToast(value: unknown): FrameToast {
  if (!isRecord(value)) throw new Error("toast must be an object");
  const { id, kind = "info", message, detail, actions = [], dismiss = "enabled" } = value;
  if (!nonEmptyString(id)) throw new Error("toast id must be a nonempty string");
  if (!TOAST_KINDS.includes(kind as ToastKind))
    throw new Error(`toast kind must be one of ${TOAST_KINDS.join(", ")}`);
  if (!nonEmptyString(message)) throw new Error("toast message must be a nonempty string");
  if (detail !== undefined && typeof detail !== "string")
    throw new Error("toast detail must be a string");
  if (
    !Array.isArray(actions) ||
    !actions.every(
      (action) => isRecord(action) && nonEmptyString(action.key) && nonEmptyString(action.label),
    )
  ) {
    throw new Error("toast actions must be { key, label } objects");
  }
  if (dismiss !== "enabled" && dismiss !== "disabled" && dismiss !== "notify")
    throw new Error('toast dismiss must be "enabled", "disabled", or a function');
  return {
    id,
    kind: kind as ToastKind,
    message,
    detail,
    actions: actions.map((action) => ({
      key: action.key as string,
      label: action.label as string,
    })),
    dismiss,
  };
}

/**
 * The host's toasts for one local frame. Ids are namespaced by plugin so plugins never collide,
 * and the frame's toasts go away with it, since their callbacks die with the frame.
 */
export function createFrameToasts(pluginId: string, post: (event: FrameToastEvent) => void) {
  const shown = new Set<string>();
  const hostId = (id: string) => `plugin:${pluginId}:${id}`;
  return {
    show(value: unknown): string {
      const toast = parseFrameToast(value);
      const id = hostId(toast.id);
      const dismiss: ToastDismiss =
        toast.dismiss === "notify"
          ? () => post({ type: "toast-event", id: toast.id, dismiss: true })
          : toast.dismiss === "disabled"
            ? "disabled"
            : "default";
      showToast({
        id,
        kind: toast.kind,
        message: toast.message,
        detail: toast.detail,
        actions: toast.actions.map(({ key, label }) => ({
          key,
          label,
          run: () => post({ type: "toast-event", id: toast.id, action: key }),
        })),
        dismiss,
      });
      shown.add(id);
      return toast.id;
    },
    dismiss(id: unknown): void {
      if (!nonEmptyString(id)) throw new Error("toast id must be a nonempty string");
      shown.delete(hostId(id));
      dismissToast(hostId(id));
    },
    dispose(): void {
      for (const id of shown) dismissToast(id);
      shown.clear();
    },
  };
}
