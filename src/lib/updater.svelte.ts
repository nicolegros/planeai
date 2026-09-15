import { listen } from "@tauri-apps/api/event";
import { updater, type AppUpdateInfo } from "./api";

export type UpdateInfo = AppUpdateInfo;

type ManualCheckStatus = "idle" | "checking" | "available" | "up_to_date" | "error";

let launchUpdateAvailable = $state<UpdateInfo | null>(null);
let manualUpdateAvailable = $state<UpdateInfo | null>(null);
let manualCheckStatus = $state<ManualCheckStatus>("idle");
let manualCheckError = $state<string | null>(null);
let installing = $state(false);
let dismissed = $state(false);
let initialized = false;

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

export async function checkForUpdates(): Promise<void> {
  if (manualCheckStatus === "checking") return;

  manualCheckStatus = "checking";
  manualCheckError = null;
  manualUpdateAvailable = null;
  try {
    const update = await updater.check();
    if (update) {
      manualUpdateAvailable = update;
      manualCheckStatus = "available";
    } else {
      manualCheckStatus = "up_to_date";
    }
  } catch (error) {
    manualCheckError = String(error);
    manualCheckStatus = "error";
  }
}

/** Clear transient Settings feedback when the Settings window closes. */
export function resetManualUpdateState() {
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

// Initialize the listener (call once at app startup)
export function initUpdateListener() {
  if (initialized) return;
  initialized = true;
  listen<UpdateInfo>("update-available", (event) => {
    launchUpdateAvailable = event.payload;
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

/** Test-only reset for module-level reactive state. */
export function _resetForTests() {
  launchUpdateAvailable = null;
  manualUpdateAvailable = null;
  manualCheckStatus = "idle";
  manualCheckError = null;
  installing = false;
  dismissed = false;
  initialized = false;
  focusFn = null;
}
