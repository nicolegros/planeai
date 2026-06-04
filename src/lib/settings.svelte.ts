import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { loadTheme } from "./theme-loader";

export type AppearanceMode = "system" | "light" | "dark";

export interface Provider {
  command: string;
  yolo_flag: string | null;
  prompt_command?: string | null;
}

export interface LifecycleHook {
  move_to: string;
}

export interface TaskManagerTemplates {
  branch?: string | null;
  name?: string | null;
  prompt?: string | null;
}

export interface TaskManager {
  get_task: string;
  move_task: string;
  list_tasks: string;
  templates?: TaskManagerTemplates | null;
  on_start?: LifecycleHook | null;
  on_notify?: LifecycleHook | null;
  on_restart?: LifecycleHook | null;
  on_complete?: LifecycleHook | null;
}

export interface AppConfig {
  appearance: {
    mode: AppearanceMode;
    theme: string;
  };
  terminal: {
    font_family: string;
    font_size: number;
    option_as_meta: boolean;
  };
  providers: Record<string, Provider>;
  default_provider: string;
  session_backend?: string | null;
  task_managers?: Record<string, TaskManager>;
  default_task_manager?: string | null;
}

let config = $state<AppConfig>({
  appearance: {
    mode: "system",
    theme: "default",
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
  typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)").matches : true,
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
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
  // Force scrollbar repaint in WebView
  document.querySelectorAll("[class*='overflow-y']").forEach((el) => {
    const htmlEl = el as HTMLElement;
    htmlEl.style.overflow = "hidden";
    requestAnimationFrame(() => { htmlEl.style.overflow = ""; });
  });
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
  const prevTheme = config.appearance.theme;
  config = { ...config, ...patch };
  if (patch.appearance) config.appearance = { ...config.appearance, ...patch.appearance };
  if (patch.terminal) config.terminal = { ...config.terminal, ...patch.terminal };
  applyDarkClass();
  await invoke("update_config", { newConfig: config });
  if (config.appearance.theme !== prevTheme) {
    loadTheme();
  }
  emit("settings-changed");
}
