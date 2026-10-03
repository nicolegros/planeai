import { updateSettings, type AppConfig } from "./settings.svelte";
import { showSnackbar } from "./snackbar.svelte";

/** Saves a change made in a settings UI; on failure the store has rolled back and the user is told. */
export async function saveSettings(patch: Partial<AppConfig>): Promise<boolean> {
  try {
    await updateSettings(patch);
    return true;
  } catch (error) {
    showSnackbar(`Failed to save settings: ${error}`, "error");
    return false;
  }
}
