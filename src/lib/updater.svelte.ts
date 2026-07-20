import { listen } from "@tauri-apps/api/event";

interface UpdatePayload {
  version: string;
  body: string | null;
}

let updateAvailable = $state<UpdatePayload | null>(null);
let installing = $state(false);
let dismissed = $state(false);
let initialized = false;

export function getUpdateState() {
  return { updateAvailable, installing, dismissed };
}

export function dismissUpdate() {
  dismissed = true;
}

export function setInstalling(value: boolean) {
  installing = value;
}

// Initialize the listener (call once at app startup)
export function initUpdateListener() {
  if (initialized) return;
  initialized = true;
  listen<UpdatePayload>("update-available", (event) => {
    updateAvailable = event.payload;
  });
}

/** Focus coordination — component registers its focus function */
let focusFn: (() => void) | null = null;
export function registerUpdateFocus(fn: () => void) {
  focusFn = fn;
}
export function unregisterUpdateFocus() {
  focusFn = null;
}
export function focusUpdateToast() {
  focusFn?.();
}
