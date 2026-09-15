import { listen } from "@tauri-apps/api/event";
import { updater, type AppUpdateInfo } from "./api";

export type UpdateInfo = AppUpdateInfo;

type ManualCheckStatus = "idle" | "checking" | "available" | "up_to_date" | "error";

let launchUpdateAvailable = $state<UpdateInfo | null>(null);
let manualUpdateAvailable = $state<UpdateInfo | null>(null);
let manualCheckStatus = $state<ManualCheckStatus>("idle");
let manualCheckError = $state<string | null>(null);
let manualCheckGeneration = 0;
let installing = $state(false);
let dismissed = $state(false);
let initialized = false;
let listenerRegistration: Promise<void> | null = null;

export function getUpdateState() {
  return { updateAvailable: launchUpdateAvailable, installing, dismissed };
}

/** State used by Settings. A manual check never drives the launch toast. */
export function getSettingsUpdateState() {
  return {
    updateAvailable:
      manualCheckStatus === "available" ? manualUpdateAvailable : launchUpdateAvailable,
    checking: manualCheckStatus === "checking",
    upToDate: manualCheckStatus === "up_to_date",
    checkError: manualCheckError,
    installing,
  };
}

export function setLaunchUpdateAvailable(update: UpdateInfo | null) {
  launchUpdateAvailable = update;
}

export async function checkForUpdates(): Promise<void> {
  if (manualCheckStatus === "checking") return;

  const generation = ++manualCheckGeneration;
  manualCheckStatus = "checking";
  manualCheckError = null;
  manualUpdateAvailable = null;
  try {
    const update = await updater.check();
    if (generation !== manualCheckGeneration) return;
    if (update) {
      manualUpdateAvailable = update;
      manualCheckStatus = "available";
    } else {
      manualCheckStatus = "up_to_date";
    }
  } catch (error) {
    if (generation !== manualCheckGeneration) return;
    manualCheckError = String(error);
    manualCheckStatus = "error";
  }
}

/** Clear transient Settings feedback when the Settings window closes. */
export function resetManualUpdateState() {
  manualCheckGeneration += 1;
  manualUpdateAvailable = null;
  manualCheckStatus = "idle";
  manualCheckError = null;
}

export function dismissUpdate() {
  dismissed = true;
}

export function setInstalling(value: boolean) {
  installing = value;
}

// Initialize the listener (call once per webview)
export function initUpdateListener(): Promise<void> {
  if (initialized) return Promise.resolve();
  if (listenerRegistration) return listenerRegistration;

  listenerRegistration = listen<UpdateInfo>("update-available", (event) => {
    setLaunchUpdateAvailable(event.payload);
  })
    .then(() => {
      initialized = true;
    })
    .catch((error) => {
      console.warn("Failed to listen for update availability:", error);
    })
    .finally(() => {
      listenerRegistration = null;
    });
  return listenerRegistration;
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

/** Test-only reset for module-level reactive state. */
export function _resetForTests() {
  launchUpdateAvailable = null;
  manualUpdateAvailable = null;
  manualCheckStatus = "idle";
  manualCheckError = null;
  manualCheckGeneration = 0;
  installing = false;
  dismissed = false;
  initialized = false;
  listenerRegistration = null;
  focusFn = null;
}
