let message = $state<string | null>(null);

export function showSnackbar(msg: string) {
  message = msg;
}

export function dismissSnackbar() {
  message = null;
}

export function getSnackbarMessage() {
  return message;
}
