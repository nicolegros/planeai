export type SnackbarType = "error" | "success";

let message = $state<string | null>(null);
let type_ = $state<SnackbarType>("error");
let dismissTimer: ReturnType<typeof setTimeout> | null = null;

export function showSnackbar(msg: string, t: SnackbarType = "error") {
  if (dismissTimer) clearTimeout(dismissTimer);
  message = msg;
  type_ = t;
  if (t === "success") {
    dismissTimer = setTimeout(() => {
      message = null;
      dismissTimer = null;
    }, 3000);
  }
}

export function dismissSnackbar() {
  message = null;
}

export function getSnackbarMessage() {
  return message;
}

export function getSnackbarType(): SnackbarType {
  return type_;
}
