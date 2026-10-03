use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Cached result of tmux availability check (runs once per process).
static TMUX_AVAILABLE: OnceLock<bool> = OnceLock::new();
static RMUX_AVAILABLE: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntegrationsConfig {
    /// Legacy Jira settings retained only until they are imported into the
    /// bundled Jira plugin. The host no longer interprets or runs them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira: Option<serde_json::Value>,
}

/// User-owned, trusted overrides for language-server discovery. Project files
/// never supply executable commands; profiles live only in PlaneAI config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LanguageServerSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<LanguageServerProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_servers: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_on_save: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LanguageServerProfile {
    pub id: String,
    pub language_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditorConfig {
    #[serde(default = "default_editor_mode")]
    pub mode: String,
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

const INVALID_EDITOR_MODE: &str = "__invalid__";

fn default_editor_mode() -> String {
    "embedded".to_string()
}

fn invalid_editor_config() -> EditorConfig {
    EditorConfig {
        mode: INVALID_EDITOR_MODE.to_string(),
        command: String::new(),
        args: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SidebarGroupBy {
    Project,
    Status,
}

/// What happens to a task's session when the post-merge prompt times out.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostMergeAction {
    Archive,
    Destroy,
    Keep,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub providers: HashMap<String, Provider>,
    pub default_provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vim_mode: Option<bool>,
    /// Written as `null` when off, so turning tasks off survives the default being on.
    #[serde(default)]
    pub task_management: Option<TaskManager>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects_base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_done_tasks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_empty_projects: Option<bool>,
    /// Top-level sidebar grouping; `None` groups by project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_group_by: Option<SidebarGroupBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_task_keys: Option<bool>,
    /// Only affects the status grouping, where rows from several projects are mixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_project_labels: Option<bool>,
    /// `None` archives, matching the frontend default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_merge_action: Option<PostMergeAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_scrollback_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback_lines: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_links: Option<bool>,
    /// Directory for durable session logs. Env var PLANEAI_SESSION_LOG_DIR takes priority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_log_dir: Option<String>,
    /// Extra directories to prepend to PATH when spawning sessions.
    /// Use for custom shim directories (e.g. `["~/.guardrails/shims"]`).
    /// Env var PLANEAI_EXTRA_PATH overrides this (colon-separated).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_path_dirs: Vec<String>,
    #[serde(
        default = "default_auto_open_review",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_open_review: Option<bool>,
    #[serde(
        default = "default_sound_enabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub sound_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrations: Option<IntegrationsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_servers: Option<LanguageServerSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<EditorConfig>,
    /// Resolved by `load`: `Some(false)` until first-run setup is finished or skipped.
    /// Configs written before this field existed count as completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboarding_completed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Appearance {
    pub mode: String,
    #[serde(default)]
    pub terminal_theme_dark: String,
    #[serde(default)]
    pub terminal_theme_light: String,
    #[serde(default)]
    pub diff_theme_dark: String,
    #[serde(default)]
    pub diff_theme_light: String,
    #[serde(default = "default_theme")]
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Terminal {
    pub font_family: String,
    pub font_size: u32,
    #[serde(default = "default_option_as_meta")]
    pub option_as_meta: bool,
}

fn default_option_as_meta() -> bool {
    cfg!(target_os = "macos")
}

fn default_auto_open_review() -> Option<bool> {
    Some(false)
}

fn default_sound_enabled() -> Option<bool> {
    Some(true)
}

fn default_theme() -> String {
    "default".to_string()
}

fn default_font_family() -> &'static str {
    if cfg!(windows) {
        "Cascadia Mono"
    } else {
        "Menlo"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yolo_flag: Option<String>,
    /// Command used when restarting an exited session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_command: Option<String>,
    /// Deprecated: migrated to auto_dispatch.autonomous_prompt_template.
    /// Kept for deserialization of old configs; stripped on save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autonomous_prompt_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskManagerTemplates {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LifecycleHook {
    pub move_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskManager {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<TaskManagerTemplates>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_start: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_notify: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_restart: Option<LifecycleHook>,
    /// Reverts the `on_notify` move once the agent works again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_resume: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_complete: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_dispatch: Option<AutoDispatchConfig>,
}

impl TaskManager {
    /// What new installs start with: the standard task statuses and no auto-dispatch.
    pub fn recommended() -> Self {
        let hook = |status: &str| {
            Some(LifecycleHook {
                move_to: status.to_string(),
            })
        };
        TaskManager {
            templates: Some(TaskManagerTemplates {
                branch: Some("{key:lower}/{title:slug}".to_string()),
                name: Some("{key:upper}: {title}".to_string()),
                prompt: Some("Implement task {key}: {title}\n\n{description}".to_string()),
            }),
            on_start: hook("in_progress"),
            on_notify: hook("in_review"),
            on_restart: hook("in_progress"),
            on_resume: hook("in_progress"),
            on_complete: hook("done"),
            auto_dispatch: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutoDispatchConfig {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_states: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    /// Wraps the rendered task prompt for autonomous (auto-dispatched) sessions.
    /// Variable: {prompt} is replaced with the rendered task prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autonomous_prompt_template: Option<String>,
}

fn default_poll_interval() -> u64 {
    30000
}
fn default_max_concurrent() -> usize {
    3
}

/// Returns the user's home directory. Checks HOME first, falls back to USERPROFILE (Windows).
pub fn home_dir() -> String {
    planeai_paths::home_dir()
}

/// Returns the config directory.
/// - Windows: %APPDATA%\<app_name>
/// - Others: $XDG_CONFIG_HOME/<app_name> or ~/.config/<app_name>
pub fn config_dir(app_name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .unwrap_or_else(|_| format!("{}\\AppData\\Roaming", home_dir()));
        PathBuf::from(base).join(app_name)
    }
    #[cfg(not(windows))]
    {
        let base =
            std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", home_dir()));
        PathBuf::from(base).join(app_name)
    }
}

impl Config {
    /// Return extra_path_dirs with tildes expanded.
    pub fn resolved_extra_path_dirs(&self) -> Vec<String> {
        self.extra_path_dirs
            .iter()
            .map(|d| planeai_core::session_launch::expand_tilde(d))
            .collect()
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "kiro".to_string(),
            Provider {
                command: "kiro-cli chat".to_string(),
                yolo_flag: Some("--trust-all-tools".to_string()),
                resume_command: Some("kiro-cli chat --resume".to_string()),
                prompt_command: Some("{prompt}".to_string()),
                autonomous_prompt_template: None, // deprecated: now on auto_dispatch
            },
        );
        providers.insert(
            "claude".to_string(),
            Provider {
                command: "claude".to_string(),
                yolo_flag: Some("--dangerously-skip-permissions".to_string()),
                resume_command: Some("claude --resume".to_string()),
                prompt_command: Some("-p {prompt}".to_string()),
                autonomous_prompt_template: None, // deprecated: now on auto_dispatch
            },
        );
        providers.insert(
            "copilot".to_string(),
            Provider {
                command: "copilot --resume".to_string(),
                yolo_flag: Some("--allow-all-tools".to_string()),
                resume_command: None,
                prompt_command: Some("{prompt}".to_string()),
                autonomous_prompt_template: None, // deprecated: now on auto_dispatch
            },
        );
        providers.insert(
            "codex".to_string(),
            Provider {
                command: "codex".to_string(),
                yolo_flag: Some("--dangerously-bypass-approvals-and-sandbox".to_string()),
                resume_command: Some("codex resume --last".to_string()),
                prompt_command: Some("{prompt}".to_string()),
                autonomous_prompt_template: None, // deprecated: now on auto_dispatch
            },
        );
        Config {
            appearance: Appearance {
                mode: "system".to_string(),
                terminal_theme_dark: String::new(),
                terminal_theme_light: String::new(),
                diff_theme_dark: String::new(),
                diff_theme_light: String::new(),
                theme: "default".to_string(),
            },
            terminal: Terminal {
                font_family: default_font_family().to_string(),
                font_size: 14,
                option_as_meta: default_option_as_meta(),
            },
            providers,
            default_provider: "kiro".to_string(),
            session_backend: None,
            vim_mode: None,
            task_management: Some(TaskManager::recommended()),
            projects_base_path: None,
            hide_done_tasks: None,
            hide_empty_projects: None,
            sidebar_group_by: None,
            hide_task_keys: None,
            hide_project_labels: None,
            post_merge_action: None,
            daemon_scrollback_bytes: None,
            scrollback_lines: None,
            web_links: None,
            session_log_dir: None,
            extra_path_dirs: Vec::new(),
            auto_open_review: Some(false),
            sound_enabled: Some(true),
            integrations: None,
            language_servers: None,
            editor: None,
            onboarding_completed: None,
        }
    }
}

/// Migrate legacy `task_managers` HashMap format to flat `task_management`.
fn migrate_legacy_task_managers(val: &mut serde_json::Value) {
    let Some(obj) = val.as_object_mut() else {
        return;
    };
    if !obj.contains_key("task_managers") || obj.contains_key("task_management") {
        return;
    }
    let Some(tms) = obj.remove("task_managers") else {
        return;
    };
    if let Some(tms_obj) = tms.as_object() {
        let default_key = obj
            .get("default_task_manager")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let key = default_key
            .as_deref()
            .or_else(|| tms_obj.keys().next().map(|s| s.as_str()));
        if let Some(k) = key {
            if let Some(val) = tms_obj.get(k) {
                obj.insert("task_management".to_string(), val.clone());
            }
        }
    }
    obj.remove("default_task_manager");
}

/// Backfill new provider fields from defaults for known providers.
/// This ensures existing config files get resume support without manual editing.
fn backfill_provider_defaults(config: &mut Config) {
    let defaults = Config::default();
    for (key, default_provider) in &defaults.providers {
        if let Some(provider) = config.providers.get_mut(key) {
            if provider.resume_command.is_none() {
                provider.resume_command = default_provider.resume_command.clone();
            }
        }
    }
}

/// Migrate `autonomous_prompt_template` from providers to `auto_dispatch`.
/// Takes the value from the auto_dispatch provider (or default provider) — first non-null wins.
/// Clears the deprecated field from all providers after migration.
/// Returns `true` if a migration was performed (config was modified).
fn migrate_autonomous_prompt_template(config: &mut Config) -> bool {
    // Check if any provider actually has a value to migrate
    let has_old_value = config
        .providers
        .values()
        .any(|p| p.autonomous_prompt_template.is_some());

    if !has_old_value {
        return false;
    }

    // Skip if auto_dispatch already has a value set
    if config
        .task_management
        .as_ref()
        .and_then(|tm| tm.auto_dispatch.as_ref())
        .and_then(|ad| ad.autonomous_prompt_template.as_ref())
        .is_some()
    {
        // Clear deprecated fields from providers
        for provider in config.providers.values_mut() {
            provider.autonomous_prompt_template = None;
        }
        return true;
    }

    // Determine which provider key to prefer for migration
    let preferred_key = config
        .task_management
        .as_ref()
        .and_then(|tm| tm.auto_dispatch.as_ref())
        .and_then(|ad| ad.provider.clone())
        .unwrap_or_else(|| config.default_provider.clone());

    // Try the preferred provider first, then fall back to any provider with a value
    let migrated_value = config
        .providers
        .get(&preferred_key)
        .and_then(|p| p.autonomous_prompt_template.clone())
        .or_else(|| {
            config
                .providers
                .values()
                .find_map(|p| p.autonomous_prompt_template.clone())
        });

    if let Some(value) = migrated_value {
        // Ensure task_management and auto_dispatch exist
        let tm = config.task_management.get_or_insert(TaskManager {
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_resume: None,
            on_complete: None,
            auto_dispatch: None,
        });
        let ad = tm.auto_dispatch.get_or_insert(AutoDispatchConfig {
            poll_interval_ms: default_poll_interval(),
            max_concurrent: default_max_concurrent(),
            provider: None,
            terminal_states: None,
            base_branch: None,
            autonomous_prompt_template: None,
        });
        ad.autonomous_prompt_template = Some(value);
    }

    // Clear deprecated fields from all providers
    for provider in config.providers.values_mut() {
        provider.autonomous_prompt_template = None;
    }

    true
}

/// Existing config files keep the defaults they were written under: tasks off when the key
/// is absent, and review auto-open on when absent or null (the frontend once read null as on).
fn keep_pre_existing_defaults(user_val: &mut serde_json::Value) {
    let Some(obj) = user_val.as_object_mut() else {
        return;
    };
    obj.entry("task_management")
        .or_insert(serde_json::Value::Null);
    if obj
        .get("auto_open_review")
        .is_none_or(serde_json::Value::is_null)
    {
        obj.insert(
            "auto_open_review".to_string(),
            serde_json::Value::Bool(true),
        );
    }
}

/// Used when an existing config.json cannot be read: behave like an existing user, never a
/// new install, so a typo does not switch on new-install defaults such as task management.
fn invalid_config_fallback() -> Config {
    Config {
        editor: Some(invalid_editor_config()),
        onboarding_completed: Some(true),
        task_management: None,
        auto_open_review: Some(true),
        ..Config::default()
    }
}

pub fn load(config_dir: &Path) -> (Config, Vec<String>) {
    let config_path = config_dir.join("config.json");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap();
        // Strip JSONC comments then parse
        let stripped = json_comments::StripComments::new(content.as_bytes());
        let mut user_val: serde_json::Value = match serde_json::from_reader(stripped) {
            Ok(v) => v,
            Err(e) => {
                let config = invalid_config_fallback();
                return (config, vec![format!("Failed to parse config.json: {e}")]);
            }
        };
        // Migrate legacy task_managers → task_management
        migrate_legacy_task_managers(&mut user_val);
        keep_pre_existing_defaults(&mut user_val);
        let default_val = serde_json::to_value(Config::default()).unwrap();
        let merged = merge_top_level(default_val, user_val);
        let mut config: Config = match serde_json::from_value(merged) {
            Ok(config) => config,
            Err(error) => {
                let config = invalid_config_fallback();
                return (
                    config,
                    vec![format!("Failed to deserialize config.json: {error}")],
                );
            }
        };
        backfill_provider_defaults(&mut config);
        config.onboarding_completed.get_or_insert(true);
        let migrated = migrate_autonomous_prompt_template(&mut config);
        if let Err(error) = validate(&config) {
            config.editor = Some(invalid_editor_config());
            return (config, vec![format!("Invalid config.json: {error}")]);
        }
        if migrated {
            // Persist the migration so the file reflects the new structure
            save(config_dir, &config).ok();
        }
        return (config, vec![]);
    }
    let config = Config {
        onboarding_completed: Some(false),
        ..Config::default()
    };
    std::fs::create_dir_all(config_dir).ok();
    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&config_path, &json).ok();
    (config, vec![])
}

pub fn save(config_dir: &Path, config: &Config) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join("config.json"), json).map_err(|e| e.to_string())
}

/// Migrate legacy SQLite settings into a config file. No-op if config already exists.
pub fn migrate_from_db(config_dir: &Path, settings: &crate::db::Settings) -> Result<(), String> {
    if config_dir.join("config.json").exists() {
        return Ok(());
    }
    // Runs on every first launch (a fresh database seeds a settings row), so it must not
    // mark onboarding done.
    let mut config = Config {
        onboarding_completed: Some(false),
        ..Config::default()
    };
    config.appearance.mode = settings.appearance_mode.clone();
    config.appearance.terminal_theme_dark = settings.terminal_theme_dark.clone();
    config.appearance.terminal_theme_light = settings.terminal_theme_light.clone();
    config.terminal.font_size = settings.font_size;
    config.terminal.font_family = settings.font_family.clone();
    save(config_dir, &config)
}

/// Normalize a base path: expand leading `~` to the user's home directory and strip trailing slash.
pub fn normalize_base_path(raw: &str) -> String {
    let expanded = if raw.starts_with("~/") {
        format!("{}{}", home_dir(), &raw[1..])
    } else if raw == "~" {
        home_dir()
    } else {
        raw.to_string()
    };
    expanded.trim_end_matches('/').to_string()
}

/// Build the full launch command for a provider, optionally appending the yolo flag.
pub fn launch_command(provider: &Provider, yolo: bool) -> String {
    match (yolo, &provider.yolo_flag) {
        (true, Some(flag)) => format!("{} {}", provider.command, flag),
        _ => provider.command.clone(),
    }
}

/// Build the command for restarting a session: use interactive resume if available, otherwise fresh launch.
pub fn restart_command_for_provider(provider: &Provider) -> String {
    if let Some(ref resume_cmd) = provider.resume_command {
        return resume_cmd.clone();
    }
    provider.command.clone()
}

/// Resolve the effective session backend: use config value if set, otherwise default to local.
pub fn resolve_backend(config: &Config) -> &str {
    match &config.session_backend {
        Some(b) => b.as_str(),
        None => "local",
    }
}

/// Check if tmux binary is available on PATH (cached — checked once per process).
#[cfg(not(windows))]
pub fn tmux_available() -> bool {
    *TMUX_AVAILABLE.get_or_init(|| executable_on_path("tmux"))
}

#[cfg(windows)]
pub fn tmux_available() -> bool {
    false
}

/// Check whether an rmux daemon binary can be found (cached per process).
///
/// Mirrors the SDK's own resolution order: the daemon executable `rmux-daemon`
/// is what actually gets spawned, with the `rmux` CLI as a fallback. Scans PATH
/// directly rather than shelling out to `which`, so it works on every platform
/// and costs only a few stats.
pub fn rmux_available() -> bool {
    *RMUX_AVAILABLE.get_or_init(|| {
        ["rmux-daemon", "rmux"]
            .iter()
            .any(|name| executable_on_path(name))
    })
}

fn executable_on_path(name: &str) -> bool {
    find_executable_in(name, &planeai_core::command::augmented_path(&[])).is_some()
}

/// Resolve `name` against a PATH-style list, or directly when it is already a path.
pub fn find_executable_in(name: &str, path: &str) -> Option<PathBuf> {
    if name.contains('/') || name.contains(std::path::MAIN_SEPARATOR) {
        let candidate = PathBuf::from(planeai_core::session_launch::expand_tilde(name));
        return candidate.is_file().then_some(candidate);
    }
    std::env::split_paths(path).find_map(|directory| {
        if directory.as_os_str().is_empty() {
            return None;
        }
        let plain = directory.join(name);
        if plain.is_file() {
            return Some(plain);
        }
        // Windows executables carry an extension.
        let extensions = std::env::var_os("PATHEXT")?;
        std::env::split_paths(&extensions).find_map(|extension| {
            let suffix = extension.to_string_lossy();
            let suffix = suffix.trim_start_matches('.');
            let candidate = directory.join(format!("{name}.{suffix}"));
            (!suffix.is_empty() && candidate.is_file()).then_some(candidate)
        })
    })
}

/// The executable a provider command launches, skipping leading `VAR=value` assignments.
pub fn provider_binary(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .find(|word| !is_env_assignment(word))
        .map(str::to_string)
}

fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Resolved binary path per agent, for every configured provider plus the built-in presets
/// (so presets missing from the config can still be offered). Uses the PATH sessions get.
pub fn detect_provider_binaries(config: &Config) -> HashMap<String, Option<String>> {
    let path = planeai_core::command::augmented_path(&config.resolved_extra_path_dirs());
    detect_provider_binaries_in(config, &path)
}

fn detect_provider_binaries_in(config: &Config, path: &str) -> HashMap<String, Option<String>> {
    let presets = Config::default().providers;
    presets
        .iter()
        .filter(|(key, _)| !config.providers.contains_key(*key))
        .chain(config.providers.iter())
        .map(|(key, provider)| {
            let resolved = provider_binary(&provider.command)
                .and_then(|binary| find_executable_in(&binary, path))
                .map(|found| found.to_string_lossy().into_owned());
            (key.clone(), resolved)
        })
        .collect()
}

/// Re-read config from disk. On success returns the new config; on any warning/error returns Err
/// (caller should keep the previous config).
pub fn refresh(config_dir: &Path) -> Result<Config, String> {
    let (config, warnings) = load(config_dir);
    if let Some(w) = warnings.into_iter().next() {
        return Err(w);
    }
    Ok(config)
}

/// Merge user config over defaults. Struct-like top-level keys (appearance, terminal)
/// get their sub-keys merged with defaults. Everything else is replaced by the overlay.
fn merge_top_level(base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    const MERGE_KEYS: &[&str] = &["appearance", "terminal"];
    match (base, overlay) {
        (serde_json::Value::Object(mut base_map), serde_json::Value::Object(overlay_map)) => {
            for (k, v) in overlay_map {
                if MERGE_KEYS.contains(&k.as_str()) {
                    if let (
                        Some(serde_json::Value::Object(base_inner)),
                        serde_json::Value::Object(over_inner),
                    ) = (base_map.get(&k), &v)
                    {
                        let mut merged = base_inner.clone();
                        for (ik, iv) in over_inner {
                            merged.insert(ik.clone(), iv.clone());
                        }
                        base_map.insert(k, serde_json::Value::Object(merged));
                        continue;
                    }
                }
                base_map.insert(k, v);
            }
            serde_json::Value::Object(base_map)
        }
        (_, overlay) => overlay,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

/// Validate editor settings supplied by Preferences or manually edited config.
pub fn validate_editor_config(editor: &EditorConfig) -> Result<(), String> {
    match editor.mode.as_str() {
        "embedded" => Ok(()),
        "terminal" | "external" => {
            if editor.command.trim().is_empty() {
                return Err("Editor executable is required".to_string());
            }
            if !editor.args.iter().any(|arg| arg.contains("{file}")) {
                return Err("Editor arguments must include {file}".to_string());
            }
            Ok(())
        }
        _ => Err(format!("Unknown editor mode: {}", editor.mode)),
    }
}

pub fn validate(config: &Config) -> Result<(), String> {
    if let Some(editor) = &config.editor {
        validate_editor_config(editor)?;
    }
    Ok(())
}
