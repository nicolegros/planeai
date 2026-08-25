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
                    resume_command: Some("claude --resume".to_string()),
                    ..Default::default()
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
        hide_empty_projects: None,
        daemon_scrollback_bytes: None,
        scrollback_lines: None,
        web_links: None,
        session_log_dir: None,
        extra_path_dirs: Vec::new(),
        auto_open_review: Some(true),
        sound_enabled: Some(true),
        integrations: None,
        wsl: None,
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
            ..Default::default()
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
        ..Default::default()
    };
    assert_eq!(launch_command(&provider, false), "kiro-cli chat");
}

#[test]
fn launch_command_appends_yolo_flag_when_yolo_true() {
    let provider = Provider {
        command: "kiro-cli chat".to_string(),
        yolo_flag: Some("--trust-all-tools".to_string()),
        ..Default::default()
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
        ..Default::default()
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
fn resolve_backend_falls_back_to_local_when_unset() {
    let config = Config::default();
    assert!(config.session_backend.is_none());
    let result = resolve_backend(&config);
    assert_eq!(result, "local");
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
        resume_command: Some("kiro-cli chat --resume".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string(&provider).unwrap();
    let parsed: Provider = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.resume_command,
        Some("kiro-cli chat --resume".to_string())
    );
}

#[test]
fn provider_resume_fields_default_to_none_when_missing() {
    let json = r#"{"command": "aider"}"#;
    let parsed: Provider = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.resume_command, None);
}

#[test]
fn default_config_kiro_provider_has_resume_fields() {
    let config = Config::default();
    let kiro = config.providers.get("kiro").unwrap();
    assert_eq!(
        kiro.resume_command,
        Some("kiro-cli chat --resume".to_string())
    );
}

#[test]
fn default_config_includes_copilot_provider() {
    let config = Config::default();
    let copilot = config.providers.get("copilot").unwrap();
    assert_eq!(copilot.command, "copilot --resume");
    assert_eq!(copilot.yolo_flag, Some("--allow-all-tools".to_string()));
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
    assert_eq!(claude.prompt_command, Some("-p {prompt}".to_string()));
}

#[test]
fn restart_command_uses_resume_command_when_available() {
    let provider = Provider {
        command: "kiro-cli chat".to_string(),
        yolo_flag: Some("--trust-all-tools".to_string()),
        resume_command: Some("kiro-cli chat --resume".to_string()),
        ..Default::default()
    };
    // Even with a provider_session_id, uses interactive resume_command
    let cmd = restart_command_for_provider(&provider);
    assert_eq!(cmd, "kiro-cli chat --resume");
}

#[test]
fn restart_command_falls_back_to_fresh_when_no_resume_command() {
    let provider = Provider {
        command: "kiro-cli chat".to_string(),
        yolo_flag: Some("--trust-all-tools".to_string()),
        ..Default::default()
    };
    let cmd = restart_command_for_provider(&provider);
    assert_eq!(cmd, "kiro-cli chat");
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
    assert_eq!(
        kiro.resume_command,
        Some("kiro-cli chat --resume".to_string())
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
    assert!(path.to_string_lossy().contains(".config") || std::env::var("XDG_CONFIG_HOME").is_ok());
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

#[test]
fn refresh_returns_updated_config_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    // Write initial config
    let mut config = Config::default();
    config.appearance.mode = "light".to_string();
    save(config_dir, &config).unwrap();

    // Modify file on disk externally
    config.appearance.mode = "dark".to_string();
    save(config_dir, &config).unwrap();

    let refreshed = refresh(config_dir).unwrap();
    assert_eq!(refreshed.appearance.mode, "dark");
}

#[test]
fn refresh_returns_error_on_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    fs::write(config_dir.join("config.json"), "not valid {{{").unwrap();

    let result = refresh(config_dir);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("parse"));
}

#[test]
fn config_without_integrations_field_deserializes() {
    let dir = tempfile::tempdir().unwrap();
    let json =
        r#"{"providers": {"kiro": {"command": "kiro-cli chat"}}, "default_provider": "kiro"}"#;
    fs::write(dir.path().join("config.json"), json).unwrap();

    let (config, warnings) = load(dir.path());
    assert!(warnings.is_empty());
    assert_eq!(config.integrations, None);
}

#[test]
fn config_with_integrations_jira_deserializes() {
    let dir = tempfile::tempdir().unwrap();
    let json = r#"{
        "providers": {"kiro": {"command": "kiro-cli chat"}},
        "default_provider": "kiro",
        "integrations": {
            "jira": {
                "site": "https://test.atlassian.net",
                "sources": {
                    "myapp": {
                        "jql": "project = MA",
                        "status_map": {"In Progress": "active"},
                        "writeback": {"on_start": "In Progress", "comment": true}
                    }
                }
            }
        }
    }"#;
    fs::write(dir.path().join("config.json"), json).unwrap();

    let (config, warnings) = load(dir.path());
    assert!(warnings.is_empty());
    let jira = config.integrations.unwrap().jira.unwrap();
    assert_eq!(jira.site, "https://test.atlassian.net");
    assert_eq!(jira.sync_interval_ms, 60_000);
    let source = jira.sources.get("myapp").unwrap();
    assert_eq!(
        source.writeback.as_ref().unwrap().on_start,
        Some("In Progress".to_string())
    );
    assert!(source.writeback.as_ref().unwrap().comment);
}
