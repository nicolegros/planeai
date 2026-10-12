import { listen } from "@tauri-apps/api/event";
import { updater, type AppUpdateInfo } from "./api";
import { dismissToast, showToast } from "./toasts.svelte";

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
let toastEnabled = false;

const UPDATE_TOAST_ID = "app-update";

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
  syncUpdateToast();
}

/** Show the launch update as a toast in this webview (the main window only). */
export function enableUpdateToast() {
  toastEnabled = true;
  syncUpdateToast();
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
  syncUpdateToast();
}

export function setInstalling(value: boolean) {
  installing = value;
  syncUpdateToast();
}

async function installLaunchUpdate(): Promise<void> {
  if (installing) return;
  setInstalling(true);
  try {
    await updater.install();
  } catch (error) {
    console.error("Failed to install update:", error);
    setInstalling(false);
  }
}

function syncUpdateToast() {
  if (!toastEnabled) return;
  const update = launchUpdateAvailable;
  if (!update || dismissed) {
    dismissToast(UPDATE_TOAST_ID);
    return;
  }
  const base = { id: UPDATE_TOAST_ID, kind: "info" as const, hiddenDuringOnboarding: true };
  if (installing) {
    showToast({ ...base, message: `Installing v${update.version}…`, dismiss: "disabled" });
  } else {
    showToast({
      ...base,
      message: `Update available: v${update.version}`,
      actions: [{ key: "i", label: "Install & Restart", run: installLaunchUpdate }],
      dismiss: dismissUpdate,
    });
  }
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
  toastEnabled = false;
}
