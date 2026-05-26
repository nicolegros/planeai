use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub providers: HashMap<String, Provider>,
    pub default_provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Appearance {
    pub mode: String,
    pub terminal_theme_dark: String,
    pub terminal_theme_light: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Terminal {
    pub font_family: String,
    pub font_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub command: String,
    pub yolo_flag: Option<String>,
}

/// Returns the config directory: $XDG_CONFIG_HOME/planeai or ~/.config/planeai
pub fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.config")
        });
    PathBuf::from(base).join("planeai")
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert("kiro".to_string(), Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
        });
        Config {
            appearance: Appearance {
                mode: "system".to_string(),
                terminal_theme_dark: "one-dark".to_string(),
                terminal_theme_light: "one-light".to_string(),
            },
            terminal: Terminal {
                font_family: "Menlo".to_string(),
                font_size: 14,
            },
            providers,
            default_provider: "kiro".to_string(),
        }
    }
}

pub fn load(config_dir: &Path) -> (Config, Vec<String>) {
    let config_path = config_dir.join("config.json");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap();
        // Strip JSONC comments then parse
        let stripped = json_comments::StripComments::new(content.as_bytes());
        let user_val: serde_json::Value = match serde_json::from_reader(stripped) {
            Ok(v) => v,
            Err(e) => {
                return (Config::default(), vec![format!("Failed to parse config.json: {e}")]);
            }
        };
        let default_val = serde_json::to_value(Config::default()).unwrap();
        let merged = merge_top_level(default_val, user_val);
        let config: Config = serde_json::from_value(merged).unwrap();
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

/// Build the full launch command for a provider, optionally appending the yolo flag.
pub fn launch_command(provider: &Provider, yolo: bool) -> String {
    match (yolo, &provider.yolo_flag) {
        (true, Some(flag)) => format!("{} {}", provider.command, flag),
        _ => provider.command.clone(),
    }
}

/// Merge user config over defaults. Struct-like top-level keys (appearance, terminal)
/// get their sub-keys merged with defaults. Everything else is replaced by the overlay.
fn merge_top_level(base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    const MERGE_KEYS: &[&str] = &["appearance", "terminal"];
    match (base, overlay) {
        (serde_json::Value::Object(mut base_map), serde_json::Value::Object(overlay_map)) => {
            for (k, v) in overlay_map {
                if MERGE_KEYS.contains(&k.as_str()) {
                    if let (Some(serde_json::Value::Object(base_inner)), serde_json::Value::Object(over_inner)) = (base_map.get(&k), &v) {
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
            },
            terminal: Terminal {
                font_family: "JetBrains Mono".to_string(),
                font_size: 16,
            },
            providers: {
                let mut m = HashMap::new();
                m.insert("claude".to_string(), Provider {
                    command: "claude".to_string(),
                    yolo_flag: Some("--dangerously-skip-permissions".to_string()),
                });
                m
            },
            default_provider: "claude".to_string(),
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
        assert_eq!(config.appearance.terminal_theme_dark, "one-dark");
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
        config.providers.insert("aider".to_string(), Provider {
            command: "aider".to_string(),
            yolo_flag: Some("--yes".to_string()),
        });
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
        };
        assert_eq!(launch_command(&provider, false), "kiro-cli chat");
    }

    #[test]
    fn launch_command_appends_yolo_flag_when_yolo_true() {
        let provider = Provider {
            command: "kiro-cli chat".to_string(),
            yolo_flag: Some("--trust-all-tools".to_string()),
        };
        assert_eq!(launch_command(&provider, true), "kiro-cli chat --trust-all-tools");
    }

    #[test]
    fn launch_command_ignores_yolo_when_no_flag() {
        let provider = Provider {
            command: "aider".to_string(),
            yolo_flag: None,
        };
        assert_eq!(launch_command(&provider, true), "aider");
    }
}
