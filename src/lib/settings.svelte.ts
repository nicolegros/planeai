import { config as configApi } from "./api";
import { emit } from "@tauri-apps/api/event";
import { applyThemeMode, loadTheme } from "./theme-loader";
import type { SidebarGroupBy } from "./sidebar-model";

export type AppearanceMode = "system" | "light" | "dark";

export interface Provider {
  command: string;
  yolo_flag: string | null;
  resume_command?: string | null;
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

export interface AutoDispatchConfig {
  poll_interval_ms?: number;
  max_concurrent?: number;
  provider?: string;
  terminal_states?: string[];
  autonomous_prompt_template?: string | null;
}

export interface TaskManager {
  templates?: TaskManagerTemplates | null;
  on_start?: LifecycleHook | null;
  on_notify?: LifecycleHook | null;
  on_resume?: LifecycleHook | null;
  on_restart?: LifecycleHook | null;
  on_complete?: LifecycleHook | null;
  auto_dispatch?: AutoDispatchConfig | null;
}

export interface LanguageServerProfile {
  id: string;
  language_id: string;
  // Empty arrays are omitted by the Rust config serializer.
  extensions?: string[];
  command: string;
  args?: string[];
  enabled?: boolean | null;
}

export interface LanguageServerSettings {
  enabled?: boolean | null;
  profiles: LanguageServerProfile[];
  max_servers?: number | null;
  format_on_save?: boolean | null;
}

export interface EditorSettings {
  mode: "embedded" | "terminal" | "external";
  command: string;
  args: string[];
}

/** A color scheme by name, or a hand-written CSS theme by file stem. */
export type ThemeSource = string | { css: string };

/** One hand-written CSS theme for both modes, or a source per mode. */
export type ThemeChoice = string | { light: ThemeSource; dark: ThemeSource };

export interface AppConfig {
  appearance: {
    mode: AppearanceMode;
    theme: ThemeChoice;
  };
  terminal: {
    font_family: string;
    font_size: number;
    option_as_meta: boolean;
  };
  providers: Record<string, Provider>;
  default_provider: string;
  session_backend?: string | null;
  vim_mode?: boolean | null;
  task_management?: TaskManager | null;
  projects_base_path?: string | null;
  hide_done_tasks?: boolean | null;
  hide_empty_projects?: boolean | null;
  sidebar_group_by?: SidebarGroupBy | null;
  hide_task_keys?: boolean | null;
  hide_project_labels?: boolean | null;
  scrollback_lines?: number | null;
  web_links?: boolean | null;
  auto_open_review?: boolean | null;
  sound_enabled?: boolean | null;
  post_merge_action?: "archive" | "destroy" | "keep" | null;
  language_servers?: LanguageServerSettings | null;
  editor?: EditorSettings | null;
  extra_path_dirs?: string[];
  /** `false` until first-run setup is finished or skipped; existing configs load as `true`. */
  onboarding_completed?: boolean | null;
}

const INITIAL_THEME: ThemeChoice = "default";

function themeKey(theme: ThemeChoice): string {
  return JSON.stringify(theme);
}

let config = $state<AppConfig>({
  appearance: {
    mode: "system",
    theme: INITIAL_THEME,
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
  applyThemeMode();
  // Force scrollbar repaint in WebView
  document.querySelectorAll("[class*='overflow-y']").forEach((el) => {
    const htmlEl = el as HTMLElement;
    htmlEl.style.overflow = "hidden";
    requestAnimationFrame(() => {
      htmlEl.style.overflow = "";
    });
  });
  window.dispatchEvent(new Event("planeai-theme-changed"));
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

/** Terminal-specific settings — use in effects that should only re-fire on terminal config changes */
export function getTerminalSettings() {
  return config.terminal;
}

/** Theme of the config the backend last confirmed; theme CSS reloads only when it changes. */
let confirmedTheme = themeKey(INITIAL_THEME);

function confirmTheme() {
  const key = themeKey(config.appearance.theme);
  if (key === confirmedTheme) return;
  confirmedTheme = key;
  loadTheme();
}

export async function loadSettings(): Promise<void> {
  config = await configApi.get();
  confirmedTheme = themeKey(config.appearance.theme);
  applyDarkClass();
}

export async function refreshSettings(): Promise<void> {
  config = await configApi.refresh();
  confirmedTheme = themeKey(config.appearance.theme);
  applyDarkClass();
  loadTheme();
  emit("settings-changed");
}

let latestUpdate = 0;

/** Applies `patch` optimistically; if the backend rejects it, shows what the backend holds and rethrows. */
export async function updateSettings(patch: Partial<AppConfig>): Promise<void> {
  const update = ++latestUpdate;
  const previous = config;
  config = { ...config, ...patch };
  if (patch.appearance) config.appearance = { ...config.appearance, ...patch.appearance };
  if (patch.terminal) config.terminal = { ...config.terminal, ...patch.terminal };
  applyDarkClass();
  try {
    await configApi.update(config);
  } catch (error) {
    // A newer update sends the whole config, this change included, and settles the state itself.
    if (update === latestUpdate) {
      const saved = await configApi.get().catch(() => previous);
      if (update === latestUpdate) {
        config = saved;
        applyDarkClass();
        confirmTheme();
      }
    }
    throw error;
  }
  confirmTheme();
  emit("settings-changed");
}
