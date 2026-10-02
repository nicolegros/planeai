import type { AppConfig } from "./settings.svelte";
import { DEFAULT_SCROLLBACK_LINES } from "./terminal-scrollback";

export type SettingsCategoryId =
  | "general"
  | "appearance"
  | "terminal"
  | "agents"
  | "sessions"
  | "tasks"
  | "editor"
  | "plugins"
  | "advanced";

export interface SettingsCategory {
  id: SettingsCategoryId;
  label: string;
  description: string;
}

export const SETTINGS_CATEGORIES: readonly SettingsCategory[] = [
  {
    id: "general",
    label: "General",
    description: "Projects, merge cleanup, notifications and updates.",
  },
  {
    id: "appearance",
    label: "Appearance",
    description: "Color mode, theme and how the sidebar presents tasks.",
  },
  {
    id: "terminal",
    label: "Terminal",
    description: "Font, keyboard and scrollback for agent terminals.",
  },
  {
    id: "agents",
    label: "Agents",
    description: "The coding agents PlaneAI launches, and which one is the default.",
  },
  {
    id: "sessions",
    label: "Sessions",
    description: "Where agent processes run and how old worktrees are cleaned up.",
  },
  {
    id: "tasks",
    label: "Tasks",
    description: "Task templates, status transitions and auto-dispatch.",
  },
  {
    id: "editor",
    label: "Editor",
    description: "How project files open, Vim keybindings and language servers.",
  },
  { id: "plugins", label: "Plugins", description: "Install, enable and update plugins." },
  {
    id: "advanced",
    label: "Advanced",
    description: "Config file, command-line tool and diagnostics.",
  },
];

export interface SettingDefinition {
  /** Also the DOM anchor and the `#hash` of a deep link. */
  id: string;
  category: SettingsCategoryId;
  section: string;
  label: string;
  description?: string;
  keywords?: readonly string[];
  macOnly?: boolean;
  /** Whether the setting is on its page for this config; hidden settings are left out of search. */
  available?: (config: AppConfig) => boolean;
  /** Effective value; present only for scalar settings that offer Reset. */
  value?: (config: AppConfig) => unknown;
  reset?: (config: AppConfig, defaults: AppConfig) => Partial<AppConfig>;
}

/** An optional top-level field whose unset state means `fallback`. Reset clears it. */
function optional<K extends keyof AppConfig>(key: K, fallback: unknown) {
  return {
    value: (config: AppConfig) => config[key] ?? fallback,
    reset: (_config: AppConfig, defaults: AppConfig) =>
      ({ [key]: defaults[key] ?? null }) as Partial<AppConfig>,
  };
}

function terminal<K extends keyof AppConfig["terminal"]>(key: K) {
  return {
    value: (config: AppConfig) => config.terminal[key],
    reset: (config: AppConfig, defaults: AppConfig) => ({
      terminal: { ...config.terminal, [key]: defaults.terminal[key] },
    }),
  };
}

function appearance<K extends keyof AppConfig["appearance"]>(key: K) {
  return {
    value: (config: AppConfig) => config.appearance[key],
    reset: (config: AppConfig, defaults: AppConfig) => ({
      appearance: { ...config.appearance, [key]: defaults.appearance[key] },
    }),
  };
}

const tasksEnabled = (config: AppConfig) => config.task_management != null;

export const SETTINGS: readonly SettingDefinition[] = [
  // General
  {
    id: "projects-folder",
    category: "general",
    section: "Projects",
    label: "Projects folder",
    description: "Default folder for the project picker and new clones.",
    keywords: ["path", "directory", "clone", "base"],
    ...optional("projects_base_path", null),
  },
  {
    id: "post-merge-action",
    category: "general",
    section: "After a merge",
    label: "If the merge prompt is ignored",
    description: "Applied 30 seconds after a pull request merges.",
    keywords: ["archive", "destroy", "keep", "cleanup", "worktree"],
    ...optional("post_merge_action", "archive"),
  },
  {
    id: "auto-open-review",
    category: "general",
    section: "Review",
    label: "Open review when an agent finishes",
    description: "Opens the Review tab when the active session's agent stops with changes.",
    keywords: ["diff", "changes"],
    ...optional("auto_open_review", false),
  },
  {
    id: "sound",
    category: "general",
    section: "Notifications",
    label: "Play a sound when an agent finishes",
    keywords: ["chime", "audio", "notification", "bell"],
    ...optional("sound_enabled", true),
  },
  {
    id: "app-updates",
    category: "general",
    section: "Updates",
    label: "PlaneAI version",
    keywords: ["update", "upgrade", "release", "install"],
  },
  // Appearance
  {
    id: "appearance-mode",
    category: "appearance",
    section: "Color",
    label: "Mode",
    keywords: ["dark", "light", "system"],
    ...appearance("mode"),
  },
  {
    id: "theme",
    category: "appearance",
    section: "Color",
    label: "Theme",
    keywords: ["colors", "palette"],
    ...appearance("theme"),
  },
  {
    id: "sidebar-group-by",
    category: "appearance",
    section: "Sidebar",
    label: "Group tasks by",
    keywords: ["status", "project"],
    ...optional("sidebar_group_by", "project"),
  },
  {
    id: "show-task-keys",
    category: "appearance",
    section: "Sidebar",
    label: "Show task keys",
    keywords: ["id", "key", "jira"],
    ...optional("hide_task_keys", false),
  },
  {
    id: "show-project-labels",
    category: "appearance",
    section: "Sidebar",
    label: "Show project labels",
    description: "Only when grouped by status.",
    ...optional("hide_project_labels", false),
  },
  {
    id: "hide-empty-projects",
    category: "appearance",
    section: "Sidebar",
    label: "Hide empty projects",
    ...optional("hide_empty_projects", false),
  },
  // Terminal
  {
    id: "font-family",
    category: "terminal",
    section: "Font",
    label: "Font family",
    keywords: ["typeface", "monospace", "nerd font"],
    ...terminal("font_family"),
  },
  {
    id: "font-size",
    category: "terminal",
    section: "Font",
    label: "Font size",
    keywords: ["text", "zoom"],
    ...terminal("font_size"),
  },
  {
    id: "option-as-meta",
    category: "terminal",
    section: "Keyboard",
    label: "Use Option as Meta",
    description: "Sends Option as Meta/Escape. Required for tmux Alt-key bindings.",
    keywords: ["alt", "macos", "keyboard"],
    macOnly: true,
    ...terminal("option_as_meta"),
  },
  {
    id: "scrollback-lines",
    category: "terminal",
    section: "Behavior",
    label: "Scrollback",
    description: "Lines kept per terminal. Applies to new terminals.",
    keywords: ["history", "buffer", "lines"],
    ...optional("scrollback_lines", DEFAULT_SCROLLBACK_LINES),
  },
  {
    id: "web-links",
    category: "terminal",
    section: "Behavior",
    label: "Clickable links",
    description: "Open URLs in terminal output by clicking them. Applies to new terminals.",
    keywords: ["url", "hyperlink", "web"],
    ...optional("web_links", true),
  },
  // Agents
  {
    id: "default-agent",
    category: "agents",
    section: "Default",
    label: "Default agent",
    description: "Used for new sessions unless a task or loop picks another.",
    keywords: ["provider", "claude", "codex", "kiro", "copilot"],
  },
  {
    id: "agents",
    category: "agents",
    section: "Agents",
    label: "Agents",
    keywords: [
      "provider",
      "command",
      "yolo",
      "resume",
      "prompt",
      "preset",
      "claude",
      "codex",
      "kiro",
      "copilot",
      "custom",
    ],
  },
  {
    id: "search-paths",
    category: "agents",
    section: "Search paths",
    label: "Extra PATH folders",
    description: "Searched before your PATH when finding agents and launching sessions.",
    keywords: ["path", "bin", "shims", "directory", "extra_path_dirs"],
  },
  // Sessions
  {
    id: "session-backend",
    category: "sessions",
    section: "Backend",
    label: "Run sessions in",
    keywords: ["tmux", "daemon", "rmux", "local", "pty", "persist"],
    ...optional("session_backend", "local"),
  },
  {
    id: "stale-worktrees",
    category: "sessions",
    section: "Maintenance",
    label: "Remove stale worktrees",
    description: "Deletes worktree folders for sessions inactive for more than 48 hours.",
    keywords: ["cleanup", "disk", "worktree"],
  },
  // Tasks
  {
    id: "task-management",
    category: "tasks",
    section: "Task management",
    label: "Task management",
    keywords: ["tasks", "lifecycle", "enable"],
  },
  {
    id: "branch-template",
    available: tasksEnabled,
    category: "tasks",
    section: "Templates",
    label: "Branch name",
    keywords: ["git", "branch", "template"],
  },
  {
    id: "session-name-template",
    available: tasksEnabled,
    category: "tasks",
    section: "Templates",
    label: "Session name",
    keywords: ["template", "title"],
  },
  {
    id: "prompt-template",
    available: tasksEnabled,
    category: "tasks",
    section: "Templates",
    label: "Initial prompt",
    keywords: ["template", "prompt"],
  },
  {
    id: "lifecycle-hooks",
    available: tasksEnabled,
    category: "tasks",
    section: "Status transitions",
    label: "Status transitions",
    keywords: [
      "hooks",
      "on_start",
      "on_notify",
      "on_resume",
      "on_restart",
      "on_complete",
      "move",
      "status",
    ],
  },
  {
    id: "auto-dispatch",
    available: tasksEnabled,
    category: "tasks",
    section: "Auto-dispatch",
    label: "Auto-dispatch",
    keywords: ["autonomous", "poll", "concurrent", "queue", "terminal states"],
  },
  // Editor
  {
    id: "file-editor",
    category: "editor",
    section: "Opening files",
    label: "Open files in",
    keywords: ["external", "vscode", "code", "zed", "nvim", "embedded", "terminal"],
  },
  {
    id: "vim-mode",
    category: "editor",
    section: "Keybindings",
    label: "Vim keybindings",
    description: "Full Vim emulation in the code editor: motions, visual mode and ex commands.",
    keywords: ["vim", "modal", "keyboard"],
    ...optional("vim_mode", true),
  },
  {
    id: "language-servers",
    category: "editor",
    section: "Language servers",
    label: "Code intelligence",
    keywords: ["lsp", "language server", "rust-analyzer", "typescript"],
  },
  {
    id: "language-server-profiles",
    category: "editor",
    section: "Language servers",
    label: "Custom profiles",
    keywords: ["lsp", "language server", "profile"],
  },
  // Plugins
  {
    id: "plugins",
    category: "plugins",
    section: "Plugins",
    label: "Installed plugins",
    keywords: ["extensions", "install", "update"],
  },
  // Advanced
  {
    id: "reload-config",
    category: "advanced",
    section: "Config file",
    label: "Reload config from disk",
    keywords: ["json", "refresh", "config.json"],
  },
  {
    id: "cli",
    category: "advanced",
    section: "Command line",
    label: "Command-line tool",
    keywords: ["cli", "planeai-cli", "shell", "symlink"],
  },
  {
    id: "logs",
    category: "advanced",
    section: "Diagnostics",
    label: "Application logs",
    keywords: ["debug", "logs", "trace"],
  },
];

const BY_ID = new Map(SETTINGS.map((setting) => [setting.id, setting]));

export function settingById(id: string): SettingDefinition | undefined {
  return BY_ID.get(id);
}

export function categoryById(id: string): SettingsCategory | undefined {
  return SETTINGS_CATEGORIES.find((category) => category.id === id);
}

export function isModified(
  setting: SettingDefinition,
  config: AppConfig,
  defaults: AppConfig | null,
): boolean {
  if (!setting.value || !defaults) return false;
  return JSON.stringify(setting.value(config)) !== JSON.stringify(setting.value(defaults));
}

export function resetPatch(
  setting: SettingDefinition,
  config: AppConfig,
  defaults: AppConfig,
): Partial<AppConfig> {
  return setting.reset?.(config, defaults) ?? {};
}

/** A plugin-contributed preferences page, keyed `pluginId:contributionId`. */
export interface PluginSettingsPage {
  key: string;
  label: string;
  pluginName: string;
  description?: string;
}

export type SettingsSearchResult =
  | { kind: "setting"; id: string; setting: SettingDefinition }
  | { kind: "plugin"; id: string; key: string; label: string; pluginName: string };

function matches(haystack: string, words: string[]): boolean {
  return words.every((word) => haystack.includes(word));
}

export function searchSettings(
  query: string,
  pluginPages: readonly PluginSettingsPage[],
  config: AppConfig,
): SettingsSearchResult[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return [];
  const words = normalized.split(/\s+/);
  const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  const scored: { result: SettingsSearchResult; score: number; order: number }[] = [];
  SETTINGS.forEach((setting, order) => {
    if ((setting.macOnly && !isMac) || setting.available?.(config) === false) return;
    const label = setting.label.toLowerCase();
    const haystack = [
      label,
      setting.description,
      setting.section,
      categoryById(setting.category)?.label,
      ...(setting.keywords ?? []),
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    if (!matches(haystack, words)) return;
    const score = label.includes(normalized) ? 0 : matches(label, words) ? 1 : 2;
    scored.push({ result: { kind: "setting", id: setting.id, setting }, score, order });
  });
  pluginPages.forEach((page, index) => {
    const haystack = [page.label, page.pluginName, page.description, "plugin"]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    if (!matches(haystack, words)) return;
    const score = page.label.toLowerCase().includes(normalized) ? 0 : 2;
    scored.push({
      result: {
        kind: "plugin",
        id: page.key,
        key: page.key,
        label: page.label,
        pluginName: page.pluginName,
      },
      score,
      order: SETTINGS.length + index,
    });
  });
  return scored.sort((a, b) => a.score - b.score || a.order - b.order).map((entry) => entry.result);
}

export type SettingsLocation =
  | { kind: "category"; id: SettingsCategoryId; settingId?: string }
  | { kind: "plugin"; key: string };

/** Reads `?section=<category>&plugin=<key>#<setting>` from the Preferences window URL. */
export function parseSettingsLocation(search: string, hash: string): SettingsLocation {
  const params = new URLSearchParams(search);
  const plugin = params.get("plugin");
  if (plugin) return { kind: "plugin", key: plugin };
  const setting = settingById(decodeURIComponent(hash.replace(/^#/, "")));
  const section = categoryById(params.get("section") ?? "")?.id;
  const id = section ?? setting?.category ?? "general";
  return setting && setting.category === id
    ? { kind: "category", id, settingId: setting.id }
    : { kind: "category", id };
}

/** The `[search, hash]` that opens the Preferences window at `location`. */
export function settingsLocationQuery(location: SettingsLocation): [string, string] {
  if (location.kind === "plugin")
    return [`?page=preferences&plugin=${encodeURIComponent(location.key)}`, ""];
  return [
    `?page=preferences&section=${location.id}`,
    location.settingId ? `#${location.settingId}` : "",
  ];
}

/** The settings page a plugin contributes, so a plugin asking for preferences lands on its own page. */
export function pluginPreferencesLocation(plugin: {
  id: string;
  ui_contributions: readonly { id: string; placement: string }[];
}): SettingsLocation | undefined {
  const contribution = plugin.ui_contributions.find(
    (candidate) => candidate.placement === "preferences",
  );
  return contribution ? { kind: "plugin", key: `${plugin.id}:${contribution.id}` } : undefined;
}
