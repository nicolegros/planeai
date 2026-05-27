import { invoke } from "@tauri-apps/api/core";

export type AppearanceMode = "system" | "light" | "dark";

export interface Provider {
  command: string;
  yolo_flag: string | null;
}

export interface AppConfig {
  appearance: {
    mode: AppearanceMode;
    terminal_theme_dark: string;
    terminal_theme_light: string;
  };
  terminal: {
    font_family: string;
    font_size: number;
    option_as_meta: boolean;
  };
  providers: Record<string, Provider>;
  default_provider: string;
  session_backend?: string | null;
}

let config = $state<AppConfig>({
  appearance: {
    mode: "system",
    terminal_theme_dark: "one-dark",
    terminal_theme_light: "one-light",
  },
  terminal: {
    font_family: "Menlo",
    font_size: 14,
    option_as_meta: true,
  },
  providers: {
    kiro: { command: "kiro-cli chat", yolo_flag: "--trust-all-tools" },
  },
  default_provider: "kiro",
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
  applyDarkClass();
}

function applyDarkClass() {
  const dark = isDark();
  document.documentElement.classList.toggle("dark", dark);
}

/** Reactive — reads $state vars so Svelte tracks it in $effect/$derived */
export function isDark(): boolean {
  if (config.appearance.mode === "dark") return true;
  if (config.appearance.mode === "light") return false;
  return systemIsDark;
}

export function getSettings(): AppConfig {
  return config;
}

export async function loadSettings(): Promise<void> {
  config = await invoke<AppConfig>("get_config");
  applyDarkClass();
}

export async function updateSettings(patch: Partial<AppConfig>): Promise<void> {
  config = { ...config, ...patch };
  if (patch.appearance) config.appearance = { ...config.appearance, ...patch.appearance };
  if (patch.terminal) config.terminal = { ...config.terminal, ...patch.terminal };
  applyDarkClass();
  await invoke("update_config", { newConfig: config });
}
