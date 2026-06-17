use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

/// Pre-compiled ANSI escape code regex (avoids recompilation per call).
static ANSI_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\x1B\[[0-9;]*m").unwrap());

/// Cached result of tmux availability check (runs once per process).
static TMUX_AVAILABLE: OnceLock<bool> = OnceLock::new();

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
    pub daemon_scrollback_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback_lines: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_mounted_terminals: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_links: Option<bool>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yolo_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_sessions_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_command: Option<String>,
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
}

fn default_poll_interval() -> u64 {
    30000
}
fn default_max_concurrent() -> usize {
    3
}

/// Returns the user's home directory. Checks HOME first, falls back to USERPROFILE (Windows).
pub fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default()
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

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "kiro".to_string(),
            Provider {
                command: "kiro-cli chat".to_string(),
                yolo_flag: Some("--trust-all-tools".to_string()),
                resume_flag: Some("--resume-id".to_string()),
                list_sessions_command: Some("kiro-cli chat --list-sessions".to_string()),
                session_id_pattern: Some("SessionId: ([a-f0-9-]+)".to_string()),
                prompt_command: Some("{prompt}".to_string()),
                autonomous_prompt_template: None,
            },
        );
        providers.insert(
            "claude".to_string(),
            Provider {
                command: "claude".to_string(),
                yolo_flag: Some("--dangerously-skip-permissions".to_string()),
                resume_flag: None,
                list_sessions_command: None,
                session_id_pattern: None,
                prompt_command: Some("-p {prompt}".to_string()),
                autonomous_prompt_template: None,
            },
        );
        providers.insert(
            "copilot".to_string(),
            Provider {
                command: "copilot --resume".to_string(),
                yolo_flag: Some("--allow-all-tools".to_string()),
                resume_flag: None,
                list_sessions_command: None,
                session_id_pattern: Some("--resume=([0-9a-f-]+)".to_string()),
                prompt_command: Some("{prompt}".to_string()),
                autonomous_prompt_template: None,
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
            daemon_scrollback_bytes: None,
            scrollback_lines: None,
            max_mounted_terminals: None,
            web_links: None,
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
            if provider.resume_flag.is_none() {
                provider.resume_flag = default_provider.resume_flag.clone();
            }
            if provider.list_sessions_command.is_none() {
                provider.list_sessions_command = default_provider.list_sessions_command.clone();
            }
            if provider.session_id_pattern.is_none() {
                provider.session_id_pattern = default_provider.session_id_pattern.clone();
            }
        }
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

/// Build a resume command: base command + resume_flag + provider_session_id.
pub fn resume_command(provider: &Provider, provider_session_id: &str) -> String {
    format!(
        "{} {} {}",
        provider.command,
        provider.resume_flag.as_ref().unwrap(),
        provider_session_id
    )
}

/// Returns the resume command if both resume_flag and provider_session_id are available.
pub fn resume_command_if_available(
    provider: &Provider,
    provider_session_id: Option<&str>,
) -> Option<String> {
    match (&provider.resume_flag, provider_session_id) {
        (Some(_), Some(id)) => Some(resume_command(provider, id)),
        _ => None,
    }
}

/// Build the command for restarting a session: resume if possible, otherwise fresh launch.
pub fn restart_command_for_provider(
    provider: &Provider,
    provider_session_id: Option<&str>,
) -> String {
    resume_command_if_available(provider, provider_session_id)
        .unwrap_or_else(|| provider.command.clone())
}

/// Parse a provider session ID from command output using a regex pattern.
/// Strips ANSI escape codes before matching. Returns the first capture group of the first match.
pub fn parse_provider_session_id(output: &str, pattern: &str) -> Option<String> {
    let stripped = ANSI_RE.replace_all(output, "");
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(&stripped)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Decide whether a discovered provider session ID should be accepted.
/// - `discovered`: the ID just found from list_sessions_command output
/// - `previous`: the previously stored provider_session_id (if any)
/// - `is_resume`: whether this launch used --resume-id
pub fn should_accept_provider_session_id(
    discovered: Option<&str>,
    previous: Option<&str>,
    is_resume: bool,
) -> bool {
    match discovered {
        None => false,
        Some(id) => match (previous, is_resume) {
            (Some(prev), true) => id == prev,
            (None, true) => true,
            (None, false) => true,
            (Some(prev), false) => id != prev,
        },
    }
}

/// Resolve the effective session backend: use config value if set, otherwise auto-detect.
pub fn resolve_backend(config: &Config) -> &str {
    match &config.session_backend {
        Some(b) => b.as_str(),
        None => {
            if tmux_available() {
                "tmux"
            } else {
                "daemon"
            }
        }
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
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_creates_default_config_when_no_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let (config, warnings) = load(config_dir);

        assert_eq!(config, Config::default());
        assert!(warnings.is_empty());
        assert!(config_dir.join("config.json").exists());
    }

    #[test]
    fn load_reads_existing_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let custom = Config {
            appearance: Appearance {
                mode: "dark".to_string(),
                terminal_theme_dark: "catppuccin".to_string(),
                terminal_theme_light: "one-light".to_string(),
                diff_theme_dark: "vs-dark".to_string(),
                diff_theme_light: "vs".to_string(),
                theme: "default".to_string(),
            },
            terminal: Terminal {
                font_family: "JetBrains Mono".to_string(),
                font_size: 16,
                option_as_meta: true,
            },
            providers: {
                let mut m = HashMap::new();
                m.insert(
                    "claude".to_string(),
                    Provider {
                        command: "claude".to_string(),
                        yolo_flag: Some("--dangerously-skip-permissions".to_string()),
                        resume_flag: None,
                        list_sessions_command: None,
                        session_id_pattern: None,
                        prompt_command: None,
                        autonomous_prompt_template: None,
                    },
                );
                m
            },
            default_provider: "claude".to_string(),
            session_backend: None,
            vim_mode: None,
            task_management: None,
            projects_base_path: None,
            pr_status: None,
            hide_done_tasks: None,
            daemon_scrollback_bytes: None,
            scrollback_lines: None,
            max_mounted_terminals: None,
            web_links: None,
        };

        let json = serde_json::to_string_pretty(&custom).unwrap();
        fs::write(config_dir.join("config.json"), &json).unwrap();

        let (config, warnings) = load(config_dir);

        assert_eq!(config, custom);
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_merges_with_defaults_when_fields_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        // Write a partial config — only appearance.mode set
        let partial = r#"{ "appearance": { "mode": "dark" } }"#;
        fs::write(config_dir.join("config.json"), partial).unwrap();

        let (config, warnings) = load(config_dir);

        // Specified field is kept
        assert_eq!(config.appearance.mode, "dark");
        // Missing fields filled from defaults
        assert_eq!(config.appearance.terminal_theme_dark, "");
        assert_eq!(config.terminal.font_size, 14);
        assert_eq!(config.default_provider, "kiro");
        assert!(config.providers.contains_key("kiro"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_returns_defaults_with_warning_on_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        fs::write(config_dir.join("config.json"), "not valid json {{{").unwrap();

        let (config, warnings) = load(config_dir);

        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("parse"));
    }

    #[test]
    fn load_parses_jsonc_with_comments() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let jsonc = r#"{
            // This is a comment
            "appearance": {
                "mode": "light" /* inline comment */
            }
        }"#;
        fs::write(config_dir.join("config.json"), jsonc).unwrap();

        let (config, warnings) = load(config_dir);

        assert_eq!(config.appearance.mode, "light");
        assert!(warnings.is_empty());
    }

    #[test]
    fn save_writes_config_as_pretty_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let config = Config::default();
        save(config_dir, &config).unwrap();

        let content = fs::read_to_string(config_dir.join("config.json")).unwrap();
        // Should be valid JSON
        let parsed: Config = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed, config);
        // Should be pretty-printed (contains newlines)
        assert!(content.contains('\n'));
    }

    #[test]
    fn round_trip_save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let mut config = Config::default();
        config.appearance.mode = "dark".to_string();
        config.terminal.font_size = 18;
        config.providers.insert(
            "aider".to_string(),
            Provider {
                command: "aider".to_string(),
                yolo_flag: Some("--yes".to_string()),
                resume_flag: None,
                list_sessions_command: None,
                session_id_pattern: None,
                prompt_command: None,
                autonomous_prompt_template: None,
            },
        );
        config.default_provider = "aider".to_string();

        save(config_dir, &config).unwrap();
        let (loaded, warnings) = load(config_dir);

        assert_eq!(loaded, config);
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_from_db_creates_config_from_legacy_settings() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let settings = crate::db::Settings {
            terminal_theme_dark: "catppuccin".to_string(),
            terminal_theme_light: "github-light".to_string(),
            font_size: 16,
            font_family: "Fira Code".to_string(),
            appearance_mode: "dark".to_string(),
        };

        migrate_from_db(config_dir, &settings).unwrap();

        let (config, warnings) = load(config_dir);
        assert!(warnings.is_empty());
        assert_eq!(config.appearance.mode, "dark");
        assert_eq!(config.appearance.terminal_theme_dark, "catppuccin");
        assert_eq!(config.appearance.terminal_theme_light, "github-light");
        assert_eq!(config.terminal.font_size, 16);
        assert_eq!(config.terminal.font_family, "Fira Code");
        // Providers should be defaults (DB didn't have provider info)
        assert!(config.providers.contains_key("kiro"));
    }

    #[test]
    fn migrate_from_db_does_nothing_when_config_exists() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        // Create an existing config
        let existing = Config::default();
        save(config_dir, &existing).unwrap();

        // Try to migrate with different settings
        let settings = crate::db::Settings {
            terminal_theme_dark: "catppuccin".to_string(),
            terminal_theme_light: "github-light".to_string(),
            font_size: 20,
            font_family: "Fira Code".to_string(),
            appearance_mode: "dark".to_string(),
        };

        migrate_from_db(config_dir, &settings).unwrap();

        // Config should be unchanged
        let (config, _) = load(config_dir);
        assert_eq!(config, existing);
    }

    #[test]
    fn launch_command_returns_base_when_yolo_false() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: None,
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        assert_eq!(launch_command(&provider, false), "kiro-cli chat");
    }

    #[test]
    fn launch_command_appends_yolo_flag_when_yolo_true() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: None,
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        assert_eq!(
            launch_command(&provider, true),
            "kiro-cli chat --trust-all-tools"
        );
    }

    #[test]
    fn launch_command_ignores_yolo_when_no_flag() {
        let provider = Provider {
            command: "aider".to_string(),
            yolo_flag: None,
            resume_flag: None,
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        assert_eq!(launch_command(&provider, true), "aider");
    }

    #[test]
    fn resolve_backend_returns_config_value_when_set() {
        let config = Config {
            session_backend: Some("daemon".to_string()),
            ..Default::default()
        };
        assert_eq!(resolve_backend(&config), "daemon");

        let config = Config {
            session_backend: Some("tmux".to_string()),
            ..Default::default()
        };
        assert_eq!(resolve_backend(&config), "tmux");
    }

    #[test]
    fn resolve_backend_falls_back_to_tmux_detection_when_unset() {
        let config = Config::default();
        assert!(config.session_backend.is_none());
        let result = resolve_backend(&config);
        // The key behavior: it returns either "tmux" or "daemon", never panics
        assert!(result == "tmux" || result == "daemon");
        // And it matches tmux_available()
        if tmux_available() {
            assert_eq!(result, "tmux");
        } else {
            assert_eq!(result, "daemon");
        }
    }

    #[test]
    fn session_backend_round_trips_through_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        // Save with session_backend = Some("daemon")
        let config = Config {
            session_backend: Some("daemon".to_string()),
            ..Default::default()
        };
        save(config_dir, &config).unwrap();
        let (loaded, _) = load(config_dir);
        assert_eq!(loaded.session_backend, Some("daemon".to_string()));

        // Save with session_backend = None (should be absent from JSON)
        let config = Config::default();
        save(config_dir, &config).unwrap();
        let content = fs::read_to_string(config_dir.join("config.json")).unwrap();
        assert!(!content.contains("session_backend"));
        let (loaded, _) = load(config_dir);
        assert_eq!(loaded.session_backend, None);
    }

    #[test]
    fn default_font_is_platform_appropriate() {
        let config = Config::default();
        if cfg!(windows) {
            assert_eq!(config.terminal.font_family, "Cascadia Mono");
        } else {
            assert_eq!(config.terminal.font_family, "Menlo");
        }
    }

    #[test]
    fn provider_resume_fields_round_trip_through_serde() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: Some("--resume-id".to_string()),
            list_sessions_command: Some("kiro-cli chat --list-sessions".to_string()),
            session_id_pattern: Some("SessionId: ([a-f0-9-]+)".to_string()),
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        let json = serde_json::to_string(&provider).unwrap();
        let parsed: Provider = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resume_flag, Some("--resume-id".to_string()));
        assert_eq!(
            parsed.list_sessions_command,
            Some("kiro-cli chat --list-sessions".to_string())
        );
        assert_eq!(
            parsed.session_id_pattern,
            Some("SessionId: ([a-f0-9-]+)".to_string())
        );
    }

    #[test]
    fn provider_resume_fields_default_to_none_when_missing() {
        let json = r#"{"command": "aider"}"#;
        let parsed: Provider = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.resume_flag, None);
        assert_eq!(parsed.list_sessions_command, None);
        assert_eq!(parsed.session_id_pattern, None);
    }

    #[test]
    fn default_config_kiro_provider_has_resume_fields() {
        let config = Config::default();
        let kiro = config.providers.get("kiro").unwrap();
        assert_eq!(kiro.resume_flag, Some("--resume-id".to_string()));
        assert_eq!(
            kiro.list_sessions_command,
            Some("kiro-cli chat --list-sessions".to_string())
        );
        assert!(kiro
            .session_id_pattern
            .as_ref()
            .unwrap()
            .contains("SessionId"));
    }

    #[test]
    fn default_config_includes_copilot_provider() {
        let config = Config::default();
        let copilot = config.providers.get("copilot").unwrap();
        assert_eq!(copilot.command, "copilot --resume");
        assert_eq!(copilot.yolo_flag, Some("--allow-all-tools".to_string()));
        assert_eq!(copilot.resume_flag, None);
        assert_eq!(
            copilot.session_id_pattern,
            Some("--resume=([0-9a-f-]+)".to_string())
        );
        assert_eq!(copilot.prompt_command, Some("{prompt}".to_string()));
    }

    #[test]
    fn default_config_includes_claude_provider() {
        let config = Config::default();
        let claude = config.providers.get("claude").unwrap();
        assert_eq!(claude.command, "claude");
        assert_eq!(
            claude.yolo_flag,
            Some("--dangerously-skip-permissions".to_string())
        );
        assert_eq!(claude.resume_flag, None);
        assert_eq!(claude.prompt_command, Some("-p {prompt}".to_string()));
    }

    #[test]
    fn resume_command_appends_flag_and_session_id() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: Some("--resume-id".to_string()),
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        let result = resume_command(&provider, "abc-123");
        assert_eq!(result, "kiro-cli chat --resume-id abc-123");
    }

    #[test]
    fn resume_command_returns_none_when_no_resume_flag() {
        let provider = Provider {
            command: "aider".to_string(),
            yolo_flag: None,
            resume_flag: None,
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        assert_eq!(
            resume_command_if_available(&provider, Some("abc-123")),
            None
        );
    }

    #[test]
    fn resume_command_returns_none_when_no_session_id() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: None,
            resume_flag: Some("--resume-id".to_string()),
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        assert_eq!(resume_command_if_available(&provider, None), None);
    }

    #[test]
    fn resume_command_if_available_returns_command_when_both_present() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: Some("--resume-id".to_string()),
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        assert_eq!(
            resume_command_if_available(&provider, Some("abc-123")),
            Some("kiro-cli chat --resume-id abc-123".to_string())
        );
    }

    #[test]
    fn parse_provider_session_id_extracts_uuid_from_output() {
        let output = "Chat sessions for /some/path:\n\nChat SessionId: f4165541-f370-4fdd-9ccd-14b103a4f712\n  12 minutes ago | some description | 203 msgs | v2\n";
        let pattern = "SessionId: ([a-f0-9-]+)";
        let result = parse_provider_session_id(output, pattern);
        assert_eq!(
            result,
            Some("f4165541-f370-4fdd-9ccd-14b103a4f712".to_string())
        );
    }

    #[test]
    fn parse_provider_session_id_returns_first_match() {
        let output = "Chat SessionId: aaaa-1111\nChat SessionId: bbbb-2222\n";
        let pattern = "SessionId: ([a-f0-9-]+)";
        let result = parse_provider_session_id(output, pattern);
        assert_eq!(result, Some("aaaa-1111".to_string()));
    }

    #[test]
    fn parse_provider_session_id_returns_none_when_no_match() {
        let output = "No sessions found\n";
        let pattern = "SessionId: ([a-f0-9-]+)";
        let result = parse_provider_session_id(output, pattern);
        assert_eq!(result, None);
    }

    #[test]
    fn parse_provider_session_id_strips_ansi_codes() {
        let output = "\x1B[38;5;141mChat SessionId: f4165541-f370-4fdd-9ccd-14b103a4f712\x1B[0m\n";
        let pattern = "SessionId: ([a-f0-9-]+)";
        let result = parse_provider_session_id(output, pattern);
        assert_eq!(
            result,
            Some("f4165541-f370-4fdd-9ccd-14b103a4f712".to_string())
        );
    }

    #[test]
    fn parse_provider_session_id_extracts_copilot_resume_id() {
        let output = "  ╭─╮╭─╮   Changes    +0 -0\n  ╰─╯╰─╯   AI Credits 0 (73h 15m 23s)\n  ▄ ▒▙ ▄   Resume     copilot --resume=a7c77286-ccfc-419b-bd1a-47f88f3e683a\n   ▀▀▀▀\n";
        let pattern = "--resume=([0-9a-f-]+)";
        let result = parse_provider_session_id(output, pattern);
        assert_eq!(
            result,
            Some("a7c77286-ccfc-419b-bd1a-47f88f3e683a".to_string())
        );
    }

    #[test]
    fn should_accept_session_id_fresh_launch_no_previous() {
        // Fresh launch, no previous ID → accept any ID
        assert!(should_accept_provider_session_id(
            Some("new-id"),
            None,
            false
        ));
    }

    #[test]
    fn should_accept_session_id_fresh_launch_with_different_id() {
        // Fresh launch, previous ID exists, new ID is different → accept
        assert!(should_accept_provider_session_id(
            Some("new-id"),
            Some("old-id"),
            false
        ));
    }

    #[test]
    fn should_reject_session_id_fresh_launch_with_same_id() {
        // Fresh launch, previous ID exists, same ID returned → reject (stale)
        assert!(!should_accept_provider_session_id(
            Some("old-id"),
            Some("old-id"),
            false
        ));
    }

    #[test]
    fn should_accept_session_id_resume_with_same_id() {
        // Resume, same ID returned → accept (expected)
        assert!(should_accept_provider_session_id(
            Some("old-id"),
            Some("old-id"),
            true
        ));
    }

    #[test]
    fn should_reject_session_id_when_none_discovered() {
        // No ID discovered → reject regardless
        assert!(!should_accept_provider_session_id(None, None, false));
        assert!(!should_accept_provider_session_id(
            None,
            Some("old-id"),
            true
        ));
    }

    #[test]
    fn should_reject_session_id_resume_with_different_id() {
        // Resume, different ID returned → reject (belongs to another session)
        assert!(!should_accept_provider_session_id(
            Some("other-id"),
            Some("old-id"),
            true
        ));
    }

    #[test]
    fn restart_command_uses_resume_when_provider_session_id_available() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: Some("--resume-id".to_string()),
            list_sessions_command: Some("kiro-cli chat --list-sessions".to_string()),
            session_id_pattern: Some("SessionId: ([a-f0-9-]+)".to_string()),
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        let cmd = restart_command_for_provider(&provider, Some("f4165541-abc"));
        assert_eq!(cmd, "kiro-cli chat --resume-id f4165541-abc");
    }

    #[test]
    fn restart_command_falls_back_to_fresh_when_no_session_id() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
            resume_flag: Some("--resume-id".to_string()),
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        let cmd = restart_command_for_provider(&provider, None);
        assert_eq!(cmd, "kiro-cli chat");
    }

    #[test]
    fn restart_command_falls_back_to_fresh_when_no_resume_flag() {
        let provider = Provider {
            command: "aider".to_string(),
            yolo_flag: None,
            resume_flag: None,
            list_sessions_command: None,
            session_id_pattern: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        };
        let cmd = restart_command_for_provider(&provider, Some("some-id"));
        assert_eq!(cmd, "aider");
    }

    #[test]
    fn load_backfills_resume_fields_for_known_providers() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        // Write an old-style config without resume fields
        let old_config = r#"{
            "providers": {
                "kiro": {
                    "command": "kiro-cli chat",
                    "yolo_flag": "--trust-all-tools"
                }
            },
            "default_provider": "kiro"
        }"#;
        fs::write(config_dir.join("config.json"), old_config).unwrap();

        let (config, warnings) = load(config_dir);
        assert!(warnings.is_empty());
        let kiro = config.providers.get("kiro").unwrap();
        assert_eq!(kiro.resume_flag, Some("--resume-id".to_string()));
        assert_eq!(
            kiro.list_sessions_command,
            Some("kiro-cli chat --list-sessions".to_string())
        );
        assert!(kiro.session_id_pattern.is_some());
    }

    #[test]
    fn load_backfills_copilot_session_id_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let old_config = r#"{
            "providers": {
                "copilot": {
                    "command": "copilot --resume",
                    "yolo_flag": "--allow-all-tools"
                }
            },
            "default_provider": "copilot"
        }"#;
        fs::write(config_dir.join("config.json"), old_config).unwrap();

        let (config, _) = load(config_dir);
        let copilot = config.providers.get("copilot").unwrap();
        assert_eq!(
            copilot.session_id_pattern,
            Some("--resume=([0-9a-f-]+)".to_string())
        );
    }

    #[test]
    fn home_dir_prefers_home_and_falls_back_to_userprofile() {
        let original_home = std::env::var("HOME").ok();

        // When HOME is set, it's returned
        std::env::set_var("HOME", "/mock/home");
        assert_eq!(home_dir(), "/mock/home");

        // When HOME is absent, falls back to USERPROFILE
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", "C:\\Users\\test");
        assert_eq!(home_dir(), "C:\\Users\\test");

        // Restore
        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        }
    }

    #[test]
    #[cfg(unix)]
    fn config_dir_uses_app_name_for_directory() {
        // Test the structure: config_dir returns <base>/<app_name>
        let path = config_dir("planeai");
        assert!(path.ends_with("planeai"));
        assert!(
            path.to_string_lossy().contains(".config") || std::env::var("XDG_CONFIG_HOME").is_ok()
        );
    }

    #[test]
    #[cfg(unix)]
    fn config_dir_isolates_dev_bundle_by_name() {
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/mock/home");
        std::env::remove_var("XDG_CONFIG_HOME");

        let path = config_dir("planeai-feat-foo");
        assert_eq!(path, PathBuf::from("/mock/home/.config/planeai-feat-foo"));

        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        }
    }

    #[test]
    #[cfg(unix)]
    fn normalize_base_path_expands_tilde() {
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", "/Users/testuser");

        let result = normalize_base_path("~/Developer");
        assert_eq!(result, "/Users/testuser/Developer");

        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        }
    }

    #[test]
    #[cfg(windows)]
    fn config_dir_uses_appdata_on_windows() {
        let appdata = std::env::var("APPDATA").expect("APPDATA must be set on Windows");
        let path = config_dir("planeai");
        assert_eq!(path, PathBuf::from(appdata).join("planeai"));
    }

    #[test]
    #[cfg(windows)]
    fn config_dir_isolates_dev_bundle_on_windows() {
        let appdata = std::env::var("APPDATA").expect("APPDATA must be set on Windows");
        let path = config_dir("planeai-feat-foo");
        assert_eq!(path, PathBuf::from(appdata).join("planeai-feat-foo"));
    }

    #[test]
    fn normalize_base_path_strips_trailing_slash() {
        let result = normalize_base_path("/Users/testuser/Developer/");
        assert_eq!(result, "/Users/testuser/Developer");
    }

    #[test]
    fn pr_status_round_trips_through_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            pr_status: Some("gh pr view {branch} --json url,state".to_string()),
            ..Config::default()
        };

        save(dir.path(), &config).unwrap();
        let (loaded, warnings) = load(dir.path());

        assert_eq!(loaded.pr_status, config.pr_status);
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_migrates_legacy_task_managers_to_task_management() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        let json = r#"{
            "providers": { "kiro": { "command": "kiro-cli chat", "yolo_flag": "--trust-all-tools" } },
            "default_provider": "kiro",
            "task_managers": {
                "kanban": {
                    "templates": { "branch": "{key:lower}/{title:slug}" },
                    "on_start": { "move_to": "in_progress" }
                }
            },
            "default_task_manager": "kanban"
        }"#;
        fs::write(config_dir.join("config.json"), json).unwrap();

        let (config, warnings) = load(config_dir);

        assert!(warnings.is_empty());
        let tm = config
            .task_management
            .expect("task_management should be migrated");
        assert_eq!(tm.on_start.unwrap().move_to, "in_progress");
        assert_eq!(
            tm.templates.unwrap().branch.unwrap(),
            "{key:lower}/{title:slug}"
        );
    }
}
