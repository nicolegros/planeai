import { updateSettings, type AppConfig } from "./settings.svelte";
import { showToast } from "./toasts.svelte";

/** Saves a change made in a settings UI; on failure the store has rolled back and the user is told. */
export async function saveSettings(patch: Partial<AppConfig>): Promise<boolean> {
  try {
    await updateSettings(patch);
    return true;
  } catch (error) {
    showToast(`Failed to save settings: ${error}`, "error");
    return false;
  }
}
