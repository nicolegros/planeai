import { config as configApi } from "./api";
import { emit } from "@tauri-apps/api/event";
import { loadTheme } from "./theme-loader";

export type AppearanceMode = "system" | "light" | "dark";

export interface Provider {
  command: string;
  yolo_flag: string | null;
  resume_flag?: string | null;
  resume_command?: string | null;
  prompt_command?: string | null;
  autonomous_prompt_template?: string | null;
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
}

export interface TaskManager {
  templates?: TaskManagerTemplates | null;
  on_start?: LifecycleHook | null;
  on_notify?: LifecycleHook | null;
  on_restart?: LifecycleHook | null;
  on_complete?: LifecycleHook | null;
  on_pr_open?: LifecycleHook | null;
  on_pr_merge?: LifecycleHook | null;
  auto_dispatch?: AutoDispatchConfig | null;
}

export interface JiraWriteback {
  on_start?: string | null;
  on_complete?: string | null;
  comment?: boolean;
}

export interface SyncSource {
  jql: string;
  status_map?: Record<string, string>;
  writeback?: JiraWriteback | null;
}

export interface JiraConfig {
  site: string;
  sync_interval_ms?: number;
  sources?: Record<string, SyncSource>;
}

export interface IntegrationsConfig {
  jira?: JiraConfig | null;
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
  vim_mode?: boolean | null;
  task_management?: TaskManager | null;
  projects_base_path?: string | null;
  pr_status?: string | null;
  hide_done_tasks?: boolean | null;
  scrollback_lines?: number | null;
  max_mounted_terminals?: number | null;
  web_links?: boolean | null;
  auto_open_review?: boolean | null;
  integrations?: IntegrationsConfig | null;
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
    requestAnimationFrame(() => {
      htmlEl.style.overflow = "";
    });
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

/** Terminal-specific settings — use in effects that should only re-fire on terminal config changes */
export function getTerminalSettings() {
  return config.terminal;
}

export async function loadSettings(): Promise<void> {
  config = await configApi.get();
  applyDarkClass();
}

export async function updateSettings(patch: Partial<AppConfig>): Promise<void> {
  const prevTheme = config.appearance.theme;
  config = { ...config, ...patch };
  if (patch.appearance) config.appearance = { ...config.appearance, ...patch.appearance };
  if (patch.terminal) config.terminal = { ...config.terminal, ...patch.terminal };
  applyDarkClass();
  await configApi.update(config);
  if (config.appearance.theme !== prevTheme) {
    loadTheme();
  }
  emit("settings-changed");
}
