import { invoke } from "@tauri-apps/api/core";

export type AppearanceMode = "system" | "light" | "dark";

interface Settings {
  terminal_theme_dark: string;
  terminal_theme_light: string;
  font_size: number;
  font_family: string;
  appearance_mode: AppearanceMode;
}

let settings = $state<Settings>({
  terminal_theme_dark: "one-dark",
  terminal_theme_light: "one-light",
  font_size: 14,
  font_family: "Menlo",
  appearance_mode: "system",
});

let systemIsDark = $state(
  typeof window !== "undefined"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : true
);

if (typeof window !== "undefined") {
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
    systemIsDark = e.matches;
    applyDarkClass();
  });
  // Apply immediately on module load based on system preference
  applyDarkClass();
}

function applyDarkClass() {
  const dark = isDark();
  document.documentElement.classList.toggle("dark", dark);
}

/** Reactive — reads $state vars so Svelte tracks it in $effect/$derived */
export function isDark(): boolean {
  if (settings.appearance_mode === "dark") return true;
  if (settings.appearance_mode === "light") return false;
  return systemIsDark;
}

export function getSettings(): Settings {
  return settings;
}

export async function loadSettings(): Promise<void> {
  settings = await invoke<Settings>("get_settings");
  applyDarkClass();
}

export async function updateSettings(patch: Partial<Settings>): Promise<void> {
  settings = { ...settings, ...patch };
  applyDarkClass();
  await invoke("update_settings", { settings });
}
