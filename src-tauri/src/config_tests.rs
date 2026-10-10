use super::*;
use std::fs;

/// Serializes tests in this file that mutate or depend on `HOME`, which is process-global.
static HOME_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_home_env() -> std::sync::MutexGuard<'static, ()> {
    HOME_ENV
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn load_creates_default_config_when_no_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();

    let (config, warnings) = load(config_dir);

    assert_eq!(
        config,
        Config {
            onboarding_completed: Some(false),
            ..Config::default()
        }
    );
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
            theme: ThemeChoice::default(),
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
        auto_open_review: Some(true),
        sound_enabled: Some(true),
        integrations: None,
        language_servers: None,
        editor: None,
        onboarding_completed: Some(true),
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

    assert_eq!(
        config.editor.as_ref().map(|editor| editor.mode.as_str()),
        Some("__invalid__")
    );
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
    config.onboarding_completed = Some(true);

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
    let existing = Config {
        onboarding_completed: Some(true),
        ..Config::default()
    };
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
fn first_launch_command_returns_base_when_yolo_false() {
    let provider = Provider {
        command: "kiro-cli chat".to_string(),
        yolo_flag: Some("--trust-all-tools".to_string()),
        ..Default::default()
    };
    assert_eq!(provider.first_launch_command(false, None), "kiro-cli chat");
}

#[test]
fn first_launch_command_appends_yolo_flag_when_yolo_true() {
    let provider = Provider {
        command: "kiro-cli chat".to_string(),
        yolo_flag: Some("--trust-all-tools".to_string()),
        ..Default::default()
    };
    assert_eq!(
        provider.first_launch_command(true, None),
        "kiro-cli chat --trust-all-tools"
    );
}

#[test]
fn first_launch_command_ignores_yolo_when_no_flag() {
    let provider = Provider {
        command: "aider".to_string(),
        yolo_flag: None,
        ..Default::default()
    };
    assert_eq!(provider.first_launch_command(true, None), "aider");
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
fn resolve_backend_selects_rmux_when_configured() {
    // The rmux backend is opt-in through config only, the way daemon was
    // introduced; the launch and attach paths key off this exact value.
    let config = Config {
        session_backend: Some(planeai_rmux::BACKEND.to_string()),
        ..Default::default()
    };
    assert_eq!(resolve_backend(&config), "rmux");
}

#[test]
fn executable_on_path_finds_a_file_in_a_path_directory() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("planeai-fake-rmux-daemon");
    fs::write(&binary, b"#!/bin/sh\n").unwrap();

    // `rmux_available` caches per process, so the reusable lookup is what is
    // worth testing: the Preferences warning depends on it finding real files.
    // The PATH is passed explicitly: mutating the process PATH breaks concurrent tests that spawn `sh`.
    let path = dir.path().to_string_lossy().into_owned();
    let found = find_executable_in("planeai-fake-rmux-daemon", &path);
    let missing = find_executable_in("planeai-definitely-not-installed", &path);

    assert_eq!(
        found,
        Some(binary),
        "a file present in a PATH directory must be found"
    );
    assert!(
        missing.is_none(),
        "an absent binary must not be reported as present"
    );
}

#[test]
fn executable_on_path_finds_a_binary_missing_from_the_processs_own_path() {
    // A GUI app launched from Finder/Dock/Spotlight is started by launchd, which
    // does not source the user's shell profile: this process's raw `PATH` can be
    // as narrow as `/usr/bin:/bin:/usr/sbin:/sbin`, missing Homebrew, cargo, and
    // every other conventional install directory a session launch would still
    // find. If the availability check used that raw PATH, it would report a
    // backend unavailable when a real launch — which goes through
    // `augmented_path` — would succeed. This reproduces that gap directly: the
    // binary sits somewhere `augmented_path` searches but the raw PATH does not.
    let _home = lock_home_env();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap();
    let cargo_bin = std::path::PathBuf::from(&home).join(".cargo").join("bin");
    fs::create_dir_all(&cargo_bin).unwrap();
    let binary = cargo_bin.join("planeai-fake-conventional-dir-binary");
    fs::write(&binary, b"#!/bin/sh\n").unwrap();

    let launchd_path = "/usr/bin:/bin:/usr/sbin:/sbin";
    let raw = find_executable_in("planeai-fake-conventional-dir-binary", launchd_path);
    let augmented = find_executable_in(
        "planeai-fake-conventional-dir-binary",
        &planeai_core::command::augmented_path(&[]),
    );
    let _ = fs::remove_file(&binary);

    assert!(
        raw.is_none(),
        "the narrow launchd PATH must not contain the binary"
    );
    assert!(
        augmented.is_some(),
        "a binary in a conventional install directory must be found even when \
         the process's own PATH does not include it"
    );
}

#[test]
fn rmux_daemon_binary_resolves_outside_the_processs_own_path() {
    // After a reboot the app is launched by launchd with only the system PATH.
    // The rmux SDK searches that raw PATH when starting its daemon, so the
    // client must hand it an absolute path found through `augmented_path`.
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("rmux-daemon");
    fs::write(&binary, b"#!/bin/sh\n").unwrap();

    let previous_path = std::env::var_os("PATH");
    let previous_extra = std::env::var_os("PLANEAI_EXTRA_PATH");
    std::env::set_var("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    std::env::set_var("PLANEAI_EXTRA_PATH", dir.path());
    let found = rmux_daemon_binary();
    match previous_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
    match previous_extra {
        Some(value) => std::env::set_var("PLANEAI_EXTRA_PATH", value),
        None => std::env::remove_var("PLANEAI_EXTRA_PATH"),
    }

    assert_eq!(found, Some(binary));
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

/// The command each built-in provider starts with on every backend: `launch` runs it on
/// daemon, rmux and tmux, and a local session's first attach runs it.
#[test]
fn every_default_provider_starts_an_interactive_session_on_the_task_prompt() {
    let config = Config::default();
    let first_launch = |key: &str| {
        config.providers[key].first_launch_command(true, Some("- Fix the user's login"))
    };
    assert_eq!(
        first_launch("kiro"),
        "kiro-cli chat --trust-all-tools -- '- Fix the user'\\''s login'"
    );
    assert_eq!(
        first_launch("claude"),
        "claude --dangerously-skip-permissions -- '- Fix the user'\\''s login'"
    );
    assert_eq!(
        first_launch("copilot"),
        "copilot --allow-all-tools --interactive='- Fix the user'\\''s login'"
    );
    assert_eq!(
        first_launch("codex"),
        "codex --dangerously-bypass-approvals-and-sandbox -- '- Fix the user'\\''s login'"
    );
    assert_eq!(
        config.providers["copilot"].first_launch_command(false, None),
        "copilot"
    );
}

#[test]
fn every_default_provider_restarts_by_resuming() {
    let config = Config::default();
    let restart = |key: &str| restart_command_for_provider(&config.providers[key]);
    assert_eq!(
        ["kiro", "claude", "copilot", "codex"].map(restart),
        [
            "kiro-cli chat --resume",
            "claude --resume",
            "copilot --continue",
            "codex resume --last"
        ]
    );
}

#[test]
fn load_keeps_stored_provider_commands_as_written() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{
            "default_provider": "claude",
            "providers": {
                "claude": { "command": "claude", "prompt_command": "-p {prompt}" },
                "copilot": { "command": "copilot --resume", "prompt_command": "{prompt}" }
            }
        }"#,
    )
    .unwrap();

    let (config, warnings) = load(dir.path());

    assert!(warnings.is_empty(), "{warnings:?}");
    let launch = |key: &str| config.providers[key].first_launch_command(false, Some("Go"));
    assert_eq!(launch("claude"), "claude -p 'Go'");
    assert_eq!(launch("copilot"), "copilot --resume 'Go'");
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
    let _home = lock_home_env();
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
    let _home = lock_home_env();
    // Test the structure: config_dir returns <base>/<app_name>
    let path = config_dir("planeai");
    assert!(path.ends_with("planeai"));
    assert!(path.to_string_lossy().contains(".config") || std::env::var("XDG_CONFIG_HOME").is_ok());
}

#[test]
#[cfg(unix)]
fn config_dir_isolates_dev_bundle_by_name() {
    let _home = lock_home_env();
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
    let _home = lock_home_env();
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
fn config_with_legacy_jira_payload_deserializes() {
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
    assert_eq!(jira["site"], "https://test.atlassian.net");
    let source = &jira["sources"]["myapp"];
    assert_eq!(source["writeback"]["on_start"], "In Progress");
    assert_eq!(source["writeback"]["comment"], true);
}

#[test]
fn editor_defaults_to_embedded_when_omitted() {
    let config = Config::default();
    assert_eq!(config.editor, None);

    let parsed: Config = serde_json::from_str(
        r#"{"appearance":{"mode":"system"},"terminal":{"font_family":"Menlo","font_size":14},"providers":{},"default_provider":"kiro"}"#,
    )
    .unwrap();
    assert_eq!(parsed.editor, None);
}

#[test]
fn editor_settings_round_trip_through_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    let config = Config {
        editor: Some(EditorConfig {
            mode: "external".to_string(),
            command: "code".to_string(),
            args: vec!["--goto".to_string(), "{file}".to_string()],
        }),
        ..Config::default()
    };

    save(config_dir, &config).unwrap();
    let (loaded, warnings) = load(config_dir);
    assert!(warnings.is_empty());
    assert_eq!(loaded.editor, config.editor);
    assert!(validate(&loaded).is_ok());
}

#[test]
fn editor_validation_rejects_invalid_non_embedded_settings() {
    let config = Config {
        editor: Some(EditorConfig {
            mode: "terminal".to_string(),
            command: "nvim".to_string(),
            args: vec![],
        }),
        ..Config::default()
    };
    assert_eq!(
        validate(&config),
        Err("Editor arguments must include {file}".to_string())
    );
}

#[test]
fn load_returns_defaults_with_warning_for_malformed_editor_shape() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    fs::write(config_dir.join("config.json"), r#"{ "editor": "code" }"#).unwrap();

    let (config, warnings) = load(config_dir);

    assert_eq!(
        config.editor.as_ref().map(|editor| editor.mode.as_str()),
        Some("__invalid__")
    );
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Failed to deserialize config.json"));
}

#[test]
fn load_marks_editor_invalid_when_a_non_editor_field_is_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    fs::write(
        config_dir.join("config.json"),
        r#"{ "terminal": { "font_size": "large" } }"#,
    )
    .unwrap();

    let (config, warnings) = load(config_dir);

    assert_eq!(
        config.editor.as_ref().map(|editor| editor.mode.as_str()),
        Some("__invalid__")
    );
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Failed to deserialize config.json"));
}
#[test]
fn refresh_rejects_invalid_editor_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path();
    fs::write(
        config_dir.join("config.json"),
        r#"{ "editor": { "mode": "terminal", "command": "", "args": [] } }"#,
    )
    .unwrap();

    let error = refresh(config_dir).unwrap_err();

    assert!(error.contains("Editor executable is required"));
}

#[test]
fn sidebar_view_options_round_trip_through_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        sidebar_group_by: Some(SidebarGroupBy::Status),
        hide_task_keys: Some(true),
        hide_project_labels: Some(false),
        ..Config::default()
    };
    save(dir.path(), &config).unwrap();

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(content.contains(r#""sidebar_group_by": "status""#));

    let (loaded, warnings) = load(dir.path());
    assert!(warnings.is_empty());
    assert_eq!(loaded.sidebar_group_by, Some(SidebarGroupBy::Status));
    assert_eq!(loaded.hide_task_keys, Some(true));
    assert_eq!(loaded.hide_project_labels, Some(false));
}

#[test]
fn sidebar_view_options_default_to_unset() {
    let config = Config::default();
    assert_eq!(config.sidebar_group_by, None);
    assert_eq!(config.hide_task_keys, None);
    assert_eq!(config.hide_project_labels, None);
    let json = serde_json::to_string(&config).unwrap();
    assert!(!json.contains("sidebar_group_by"));
}

#[test]
fn unknown_sidebar_group_by_is_reported_as_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{ "sidebar_group_by": "statu" }"#,
    )
    .unwrap();

    let (_, warnings) = load(dir.path());

    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("unknown variant `statu`"),
        "{}",
        warnings[0]
    );
}

#[test]
fn first_launch_requires_onboarding_and_persists_the_flag() {
    let dir = tempfile::tempdir().unwrap();

    let (config, _) = load(dir.path());

    assert_eq!(config.onboarding_completed, Some(false));
    let raw = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(raw.contains("\"onboarding_completed\": false"));
    // A relaunch before finishing onboarding must still show it.
    let (relaunched, _) = load(dir.path());
    assert_eq!(relaunched.onboarding_completed, Some(false));
}

#[test]
fn existing_config_without_flag_counts_as_onboarded() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"default_provider": "claude"}"#,
    )
    .unwrap();

    let (config, warnings) = load(dir.path());

    assert!(warnings.is_empty());
    assert_eq!(config.onboarding_completed, Some(true));
}

#[test]
fn completed_onboarding_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let (mut config, _) = load(dir.path());
    config.onboarding_completed = Some(true);
    save(dir.path(), &config).unwrap();

    let (reloaded, _) = load(dir.path());

    assert_eq!(reloaded.onboarding_completed, Some(true));
}

#[test]
fn unparseable_config_does_not_trigger_onboarding() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config.json"), "{ not json").unwrap();

    let (config, warnings) = load(dir.path());

    assert!(!warnings.is_empty());
    assert_eq!(config.onboarding_completed, Some(true));
}

#[test]
fn first_launch_through_db_migration_still_requires_onboarding() {
    // Startup runs `migrate_from_db` before `load`, and a fresh database always seeds a
    // settings row, so this is the real first-launch path, not only a legacy one.
    let dir = tempfile::tempdir().unwrap();
    let settings = crate::db::Settings {
        terminal_theme_dark: String::new(),
        terminal_theme_light: String::new(),
        font_size: 14,
        font_family: "Menlo".to_string(),
        appearance_mode: "system".to_string(),
    };
    migrate_from_db(dir.path(), &settings).unwrap();

    let (config, _) = load(dir.path());

    assert_eq!(config.onboarding_completed, Some(false));
}

#[test]
fn defaults_leave_onboarding_unset() {
    assert_eq!(Config::default().onboarding_completed, None);
}

#[test]
fn provider_binary_takes_the_first_command_word() {
    assert_eq!(
        provider_binary("kiro-cli chat").as_deref(),
        Some("kiro-cli")
    );
    assert_eq!(provider_binary("  claude  ").as_deref(), Some("claude"));
    assert_eq!(
        provider_binary("FOO=1 BAR=x codex --full-auto").as_deref(),
        Some("codex")
    );
    assert_eq!(
        provider_binary("/opt/tools/agent run").as_deref(),
        Some("/opt/tools/agent")
    );
    assert_eq!(provider_binary("   "), None);
    assert_eq!(provider_binary("A=1"), None);
}

#[cfg(not(windows))]
#[test]
fn find_executable_in_searches_each_path_entry() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-agent");
    fs::write(&bin, "#!/bin/sh\n").unwrap();
    let path = format!("/nonexistent-planeai-dir:{}", dir.path().display());

    assert_eq!(find_executable_in("fake-agent", &path), Some(bin));
    assert_eq!(find_executable_in("missing-agent", &path), None);
}

#[cfg(windows)]
#[test]
fn find_executable_in_prefers_a_runnable_shim_on_windows() {
    // npm installs an extensionless shell script beside the `.cmd` shim Windows can run.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("claude"), "#!/bin/sh\n").unwrap();
    let shim = dir.path().join("claude.cmd");
    fs::write(&shim, "@echo off\r\n").unwrap();
    let path = dir.path().display().to_string();
    let found = find_executable_in("claude", &path).unwrap();
    assert!(found
        .to_string_lossy()
        .eq_ignore_ascii_case(&shim.to_string_lossy()));
}

#[cfg(not(windows))]
#[test]
fn find_executable_in_accepts_explicit_paths() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("agent");
    fs::write(&bin, "").unwrap();
    let explicit = bin.to_string_lossy().into_owned();

    assert_eq!(find_executable_in(&explicit, ""), Some(bin.clone()));
    assert_eq!(find_executable_in(&format!("{explicit}-nope"), ""), None);
}

#[cfg(not(windows))]
#[test]
fn detect_provider_binaries_covers_configured_and_preset_agents() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("my-agent"), "").unwrap();
    let mut config = Config::default();
    config.providers.remove("kiro");
    config.providers.insert(
        "custom".to_string(),
        Provider {
            command: "my-agent --fast".to_string(),
            yolo_flag: None,
            resume_command: None,
            prompt_command: None,
            autonomous_prompt_template: None,
        },
    );
    let path = dir.path().to_string_lossy().into_owned();

    let found = detect_provider_binaries_in(&config, &path);

    assert_eq!(
        found.get("custom").cloned().flatten(),
        Some(dir.path().join("my-agent").to_string_lossy().into_owned())
    );
    // Presets removed from config are still reported so they can be offered.
    assert_eq!(found.get("kiro"), Some(&None));
    assert!(found.contains_key("claude"));
    assert!(found.contains_key("codex"));
    assert!(found.contains_key("copilot"));
}

#[test]
fn post_merge_action_survives_a_settings_update_round_trip() {
    // `update_config` deserializes the frontend payload into `Config`; an unknown field is
    // silently dropped, which used to discard the user's post-merge choice.
    let mut payload = serde_json::to_value(Config::default()).unwrap();
    payload["post_merge_action"] = serde_json::json!("destroy");
    let config: Config = serde_json::from_value(payload).unwrap();
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), &config).unwrap();

    let (reloaded, warnings) = load(dir.path());

    assert!(warnings.is_empty());
    assert_eq!(reloaded.post_merge_action, Some(PostMergeAction::Destroy));
}

#[test]
fn unknown_post_merge_action_is_reported_as_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"post_merge_action": "explode"}"#,
    )
    .unwrap();

    let (_, warnings) = load(dir.path());

    assert_eq!(warnings.len(), 1);
}

#[test]
fn new_installs_start_with_review_auto_open_off_and_tasks_on() {
    let dir = tempfile::tempdir().unwrap();

    let (config, _) = load(dir.path());

    assert_eq!(config.auto_open_review, Some(false));
    assert_eq!(config.task_management, Some(TaskManager::recommended()));
}

#[test]
fn existing_config_without_task_management_keeps_tasks_off() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"default_provider": "kiro"}"#,
    )
    .unwrap();

    let (config, warnings) = load(dir.path());

    assert!(warnings.is_empty());
    assert_eq!(config.task_management, None);
}

#[test]
fn turning_task_management_off_persists() {
    let dir = tempfile::tempdir().unwrap();
    let (mut config, _) = load(dir.path());
    config.task_management = None;
    save(dir.path(), &config).unwrap();

    let (reloaded, _) = load(dir.path());

    assert_eq!(reloaded.task_management, None);
}

#[test]
fn recommended_task_management_moves_tasks_through_the_standard_statuses() {
    let tm = TaskManager::recommended();
    fn hook(h: &Option<LifecycleHook>) -> Option<&str> {
        h.as_ref().map(|h| h.move_to.as_str())
    }

    assert_eq!(hook(&tm.on_start), Some("in_progress"));
    assert_eq!(hook(&tm.on_notify), Some("in_review"));
    assert_eq!(hook(&tm.on_resume), Some("in_progress"));
    assert_eq!(hook(&tm.on_restart), Some("in_progress"));
    assert_eq!(hook(&tm.on_complete), Some("done"));
    assert!(tm.auto_dispatch.is_none());
    let templates = tm.templates.unwrap();
    assert_eq!(
        templates.branch.as_deref(),
        Some("{key:lower}/{title:slug}")
    );
    assert_eq!(templates.name.as_deref(), Some("{key:upper}: {title}"));
}

#[test]
fn existing_config_without_auto_open_review_keeps_it_on() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"default_provider": "kiro"}"#,
    )
    .unwrap();

    let (config, _) = load(dir.path());

    assert_eq!(config.auto_open_review, Some(true));
}

#[test]
fn existing_config_with_null_auto_open_review_keeps_it_on() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"auto_open_review": null}"#,
    )
    .unwrap();

    let (config, _) = load(dir.path());

    assert_eq!(config.auto_open_review, Some(true));
}

#[test]
fn broken_config_falls_back_to_existing_user_behavior() {
    for content in ["{ not json", r#"{"post_merge_action": "explode"}"#] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), content).unwrap();

        let (config, warnings) = load(dir.path());

        assert_eq!(warnings.len(), 1, "{content}");
        assert_eq!(config.task_management, None, "{content}");
        assert_eq!(config.auto_open_review, Some(true), "{content}");
        assert_eq!(config.onboarding_completed, Some(true), "{content}");
    }
}

#[test]
fn provider_keys_with_a_colon_are_reserved_for_plugins() {
    // A hand-edited config still loads unchanged, so nothing the user wrote is lost.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{ "providers": { "team:claude": { "command": "claude" } } }"#,
    )
    .unwrap();
    let (config, warnings) = load(dir.path());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(config.providers.contains_key("team:claude"));
    // Saving asks for it to be renamed first.
    assert!(validate(&config)
        .unwrap_err()
        .contains("reserved for plugin providers"));
}

#[test]
fn appearance_theme_accepts_a_css_theme_name() {
    let json = r#"{"mode": "dark", "theme": "one"}"#;
    let appearance: Appearance = serde_json::from_str(json).unwrap();
    assert_eq!(appearance.theme, ThemeChoice::Css("one".to_string()));
    let saved = serde_json::to_value(&appearance).unwrap();
    assert_eq!(saved["theme"], serde_json::json!("one"));
}

#[test]
fn appearance_theme_accepts_a_light_and_dark_scheme_pair() {
    let json = r#"{"mode": "system", "theme": {"light": "A", "dark": "B"}}"#;
    let appearance: Appearance = serde_json::from_str(json).unwrap();
    assert_eq!(
        appearance.theme,
        ThemeChoice::Schemes {
            light: "A".to_string(),
            dark: "B".to_string()
        }
    );
    let saved = serde_json::to_value(&appearance).unwrap();
    assert_eq!(
        saved["theme"],
        serde_json::json!({"light": "A", "dark": "B"})
    );
}

#[test]
fn appearance_theme_defaults_to_the_default_css_theme() {
    let appearance: Appearance = serde_json::from_str(r#"{"mode": "system"}"#).unwrap();
    assert_eq!(appearance.theme, ThemeChoice::Css("default".to_string()));
}
