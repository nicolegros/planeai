import { config as configApi } from "../../lib/api";
import type { AppConfig, EditorSettings } from "../../lib/settings.svelte";
import type { SettingsLocation } from "../../lib/settings-registry";

const FLASH_MS = 1400;

/** State shared by the Preferences window shell and its pages. */
export const settingsWindow = $state({
  location: { kind: "category", id: "general" } as SettingsLocation,
  query: "",
  flashId: null as string | null,
  /** `Config::default()` from the backend; `null` until loaded, which hides Reset. */
  defaults: null as AppConfig | null,
  /** Unsaved file-editor edits, kept across page switches; cleared when config reloads. */
  editorDraft: null as EditorSettings | null,
});

let flashTimer: ReturnType<typeof setTimeout> | undefined;

export function navigateSettings(location: SettingsLocation) {
  settingsWindow.query = "";
  settingsWindow.location = location;
  if (location.kind === "category" && location.settingId) revealSetting(location.settingId);
}

/** Scrolls the setting into view once its page has rendered, then highlights it briefly. */
function revealSetting(id: string) {
  clearTimeout(flashTimer);
  settingsWindow.flashId = id;
  requestAnimationFrame(() => {
    document
      .getElementById(`setting-${id}`)
      ?.scrollIntoView({ block: "center", behavior: "smooth" });
  });
  flashTimer = setTimeout(() => {
    settingsWindow.flashId = null;
  }, FLASH_MS);
}

/** Backend defaults, fetched on first use and retried on later calls if that fetch failed. */
export async function loadDefaults(): Promise<AppConfig | null> {
  if (!settingsWindow.defaults) {
    try {
      settingsWindow.defaults = await configApi.defaults();
    } catch (error) {
      console.warn("Failed to load config defaults:", error);
    }
  }
  return settingsWindow.defaults;
}
