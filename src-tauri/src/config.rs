use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Cached result of tmux availability check (runs once per process).
static TMUX_AVAILABLE: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntegrationsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jira: Option<planeai_jira::config::JiraConfig>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_management: Option<TaskManager>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects_base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_done_tasks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_empty_projects: Option<bool>,
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
    Some(true)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_complete: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_pr_open: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_pr_merge: Option<LifecycleHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_dispatch: Option<AutoDispatchConfig>,
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
            task_management: None,
            projects_base_path: None,
            pr_status: None,
            hide_done_tasks: None,
            hide_empty_projects: None,
            daemon_scrollback_bytes: None,
            scrollback_lines: None,
            web_links: None,
            session_log_dir: None,
            extra_path_dirs: Vec::new(),
            auto_open_review: Some(true),
            sound_enabled: Some(true),
            integrations: None,
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
            on_complete: None,
            on_pr_open: None,
            on_pr_merge: None,
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

pub fn load(config_dir: &Path) -> (Config, Vec<String>) {
    let config_path = config_dir.join("config.json");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap();
        // Strip JSONC comments then parse
        let stripped = json_comments::StripComments::new(content.as_bytes());
        let mut user_val: serde_json::Value = match serde_json::from_reader(stripped) {
            Ok(v) => v,
            Err(e) => {
                return (
                    Config::default(),
                    vec![format!("Failed to parse config.json: {e}")],
                );
            }
        };
        // Migrate legacy task_managers → task_management
        migrate_legacy_task_managers(&mut user_val);
        let default_val = serde_json::to_value(Config::default()).unwrap();
        let merged = merge_top_level(default_val, user_val);
        let mut config: Config = serde_json::from_value(merged).unwrap();
        backfill_provider_defaults(&mut config);
        let migrated = migrate_autonomous_prompt_template(&mut config);
        if migrated {
            // Persist the migration so the file reflects the new structure
            save(config_dir, &config).ok();
        }
        return (config, vec![]);
    }
    let config = Config::default();
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
    let mut config = Config::default();
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
    *TMUX_AVAILABLE.get_or_init(|| {
        std::process::Command::new("which")
            .arg("tmux")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

#[cfg(windows)]
pub fn tmux_available() -> bool {
    false
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
