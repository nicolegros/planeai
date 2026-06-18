//! Behavior parity tests for session launch.
//!
//! Proves that both Tauri-style and Iced-style callers produce equivalent
//! backend launch requests through the same shared prepare_session function.

use std::collections::HashMap;
use std::path::PathBuf;

use planeai_core::session_launch::{
    expand_tilde, load_launch_config, prepare_session, resolve_from_config, CreateSessionError,
    CreateSessionRequest, LaunchConfig, ProviderConfig, SessionLaunchOverrides, SessionTarget,
};

fn tauri_style_request(cwd: PathBuf) -> CreateSessionRequest {
    CreateSessionRequest {
        session_id: "tauri-uuid-1234".to_string(),
        project_cwd: cwd,
        session_target: SessionTarget::Daemon,
        agent_command: "kiro-cli chat --trust-all-tools".to_string(),
        env: HashMap::new(),
        extra_path_dirs: vec!["/custom/shims".to_string()],
        cols: 80,
        rows: 24,
        durable_logs: true,
    }
}

fn iced_style_request(cwd: PathBuf) -> CreateSessionRequest {
    CreateSessionRequest {
        session_id: "iced-0-12345".to_string(),
        project_cwd: cwd,
        session_target: SessionTarget::Daemon,
        agent_command: "kiro-cli chat --trust-all-tools".to_string(),
        env: HashMap::new(),
        extra_path_dirs: vec!["/custom/shims".to_string()],
        cols: 80,
        rows: 24,
        durable_logs: true,
    }
}

#[test]
fn same_cwd() {
    let cwd = std::env::temp_dir();
    let tauri = prepare_session(&tauri_style_request(cwd.clone())).unwrap();
    let iced = prepare_session(&iced_style_request(cwd.clone())).unwrap();
    assert_eq!(tauri.cwd, iced.cwd);
    assert_eq!(tauri.cwd, cwd);
}

#[test]
fn same_agent_command_resolution() {
    let cwd = std::env::temp_dir();
    let tauri = prepare_session(&tauri_style_request(cwd.clone())).unwrap();
    let iced = prepare_session(&iced_style_request(cwd)).unwrap();
    assert_eq!(tauri.program, iced.program);
    assert_eq!(tauri.args, iced.args);
    assert_eq!(tauri.command_label, iced.command_label);
}

#[test]
fn same_augmented_path() {
    let cwd = std::env::temp_dir();
    let tauri = prepare_session(&tauri_style_request(cwd.clone())).unwrap();
    let iced = prepare_session(&iced_style_request(cwd)).unwrap();
    assert_eq!(tauri.env["PATH"], iced.env["PATH"]);
    assert!(tauri.env["PATH"].contains("/custom/shims"));
}

#[test]
fn same_env_propagation() {
    let cwd = std::env::temp_dir();
    let tauri = prepare_session(&tauri_style_request(cwd.clone())).unwrap();
    let iced = prepare_session(&iced_style_request(cwd)).unwrap();
    // Both get TERM
    assert_eq!(tauri.env["TERM"], "xterm-256color");
    assert_eq!(iced.env["TERM"], "xterm-256color");
    // Both get PLANEAI_SESSION_ID (with their own session_id)
    assert_eq!(tauri.env["PLANEAI_SESSION_ID"], "tauri-uuid-1234");
    assert_eq!(iced.env["PLANEAI_SESSION_ID"], "iced-0-12345");
}

#[test]
fn same_session_target_daemon() {
    let cwd = std::env::temp_dir();
    let tauri = prepare_session(&tauri_style_request(cwd.clone())).unwrap();
    let iced = prepare_session(&iced_style_request(cwd)).unwrap();
    assert_eq!(tauri.target, SessionTarget::Daemon);
    assert_eq!(iced.target, SessionTarget::Daemon);
}

#[test]
fn same_command_label() {
    let cwd = std::env::temp_dir();
    let tauri = prepare_session(&tauri_style_request(cwd.clone())).unwrap();
    let iced = prepare_session(&iced_style_request(cwd)).unwrap();
    assert_eq!(tauri.command_label, "kiro-cli chat --trust-all-tools");
    assert_eq!(iced.command_label, "kiro-cli chat --trust-all-tools");
}

#[test]
fn same_error_for_invalid_cwd() {
    let bad_cwd = PathBuf::from("/nonexistent/xyz/abc");
    let tauri_err = prepare_session(&tauri_style_request(bad_cwd.clone())).unwrap_err();
    let iced_err = prepare_session(&iced_style_request(bad_cwd)).unwrap_err();
    assert!(matches!(tauri_err, CreateSessionError::InvalidCwd(_)));
    assert!(matches!(iced_err, CreateSessionError::InvalidCwd(_)));
}

#[test]
fn same_error_for_empty_command() {
    let cwd = std::env::temp_dir();
    let mut req = tauri_style_request(cwd.clone());
    req.agent_command = "".to_string();
    let tauri_err = prepare_session(&req).unwrap_err();

    let mut req = iced_style_request(cwd);
    req.agent_command = "   ".to_string();
    let iced_err = prepare_session(&req).unwrap_err();

    assert!(matches!(tauri_err, CreateSessionError::CommandEmpty));
    assert!(matches!(iced_err, CreateSessionError::CommandEmpty));
}

#[test]
fn tmux_target_is_explicit_not_default() {
    // Both callers explicitly set SessionTarget — it's never auto-selected
    let cwd = std::env::temp_dir();
    let req = CreateSessionRequest {
        session_id: "test".to_string(),
        project_cwd: cwd,
        session_target: SessionTarget::Tmux,
        agent_command: "bash".to_string(),
        env: HashMap::new(),
        extra_path_dirs: vec![],
        cols: 80,
        rows: 24,
        durable_logs: false,
    };
    let result = prepare_session(&req).unwrap();
    assert_eq!(result.target, SessionTarget::Tmux);
}

#[test]
fn durable_log_setting_propagates() {
    let cwd = std::env::temp_dir();
    let mut req = tauri_style_request(cwd);
    req.durable_logs = true;
    let result = prepare_session(&req).unwrap();
    // The prepare function resolves env/command — durable_logs is pass-through
    // Callers check it. Verify it doesn't affect the result env incorrectly.
    assert!(!result.env.contains_key("PLANEAI_SESSION_LOG_DIR"));
}

// ─── Config resolution tests ─────────────────────────────────────────────────

#[test]
fn config_default_agent_command_resolves() {
    let mut config = LaunchConfig::default();
    config.providers.insert(
        "test".to_string(),
        ProviderConfig {
            command: "test-agent run".to_string(),
            yolo_flag: None,
        },
    );
    config.default_provider = "test".to_string();

    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert_eq!(resolved.command_label, "test-agent run");
}

#[test]
fn cli_agent_command_overrides_config() {
    let config = LaunchConfig::default();
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        agent_command: Some("custom-agent --fast".to_string()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert_eq!(resolved.command_label, "custom-agent --fast");
}

#[test]
fn config_extra_path_dirs_applied() {
    let config = LaunchConfig {
        extra_path_dirs: vec!["/config/bin".to_string()],
        ..Default::default()
    };

    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert!(resolved
        .request
        .extra_path_dirs
        .contains(&"/config/bin".to_string()));
}

#[test]
fn cli_extra_path_dirs_augment_config() {
    let config = LaunchConfig {
        extra_path_dirs: vec!["/config/bin".to_string()],
        ..Default::default()
    };

    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        extra_path_dirs: vec!["/cli/bin".to_string()],
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert!(resolved
        .request
        .extra_path_dirs
        .contains(&"/config/bin".to_string()));
    assert!(resolved
        .request
        .extra_path_dirs
        .contains(&"/cli/bin".to_string()));
}

#[test]
fn missing_config_falls_back_to_cli() {
    let config = LaunchConfig {
        providers: HashMap::new(),
        default_provider: "nonexistent".to_string(),
        ..Default::default()
    };
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        agent_command: Some("fallback-agent".to_string()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert_eq!(resolved.command_label, "fallback-agent");
}

#[test]
fn missing_config_and_cli_returns_error() {
    let config = LaunchConfig {
        providers: HashMap::new(),
        default_provider: "nonexistent".to_string(),
        ..Default::default()
    };
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let err = resolve_from_config(&config, &overrides).unwrap_err();
    assert!(matches!(err, CreateSessionError::CommandEmpty));
}

#[test]
fn daemon_target_is_default() {
    let config = LaunchConfig::default();
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert_eq!(resolved.request.session_target, SessionTarget::Daemon);
}

#[test]
fn tmux_target_explicit_via_config() {
    let config = LaunchConfig {
        session_backend: Some("tmux".to_string()),
        ..Default::default()
    };

    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert_eq!(resolved.request.session_target, SessionTarget::Tmux);
}

#[test]
fn auto_approve_adds_yolo_flag() {
    let mut config = LaunchConfig::default();
    config.providers.insert(
        "test".to_string(),
        ProviderConfig {
            command: "agent".to_string(),
            yolo_flag: Some("--yolo".to_string()),
        },
    );
    config.default_provider = "test".to_string();

    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        auto_approve: true,
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert_eq!(resolved.command_label, "agent --yolo");
}

// ─── JSONC, merge, session_log_dir, tilde expansion tests ────────────────────

#[test]
fn jsonc_config_loads_successfully() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    std::fs::write(
        &config_path,
        r#"{
        // This is a comment
        "default_provider": "test-agent",
        "providers": {
            "test-agent": {
                "command": "my-agent run" /* inline comment */
            }
        }
    }"#,
    )
    .unwrap();
    let config = load_launch_config(&config_path).unwrap();
    assert_eq!(config.default_provider, "test-agent");
    assert_eq!(config.providers["test-agent"].command, "my-agent run");
}

#[test]
fn partial_config_merges_over_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    // Only override default_provider — providers should come from defaults
    std::fs::write(&config_path, r#"{ "default_provider": "claude" }"#).unwrap();
    let config = load_launch_config(&config_path).unwrap();
    assert_eq!(config.default_provider, "claude");
    // Default providers should still exist from merge
    assert!(config.providers.contains_key("kiro"));
    assert!(config.providers.contains_key("claude"));
}

#[test]
fn session_log_dir_from_config_propagates() {
    // Only test when env var is not set (env takes priority over config)
    if std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok() {
        return; // env var overrides — can't test config propagation
    }
    let config = LaunchConfig {
        session_log_dir: Some("/tmp/test-logs".to_string()),
        ..Default::default()
    };
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    assert!(resolved.request.durable_logs);
    assert_eq!(resolved.session_log_dir, Some("/tmp/test-logs".to_string()));
}

#[test]
fn session_log_dir_tilde_expanded() {
    if std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok() {
        return; // env var overrides — can't test config propagation
    }
    let config = LaunchConfig {
        session_log_dir: Some("~/planeai-logs".to_string()),
        ..Default::default()
    };
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    let dir = resolved.session_log_dir.unwrap();
    assert!(!dir.starts_with("~"), "tilde should be expanded: {dir}");
    assert!(dir.contains("planeai-logs"));
}

#[test]
fn extra_path_dirs_tilde_expanded() {
    let config = LaunchConfig {
        extra_path_dirs: vec!["~/.local/bin".to_string()],
        ..Default::default()
    };
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let resolved = resolve_from_config(&config, &overrides).unwrap();
    for dir in &resolved.request.extra_path_dirs {
        assert!(
            !dir.starts_with("~"),
            "tilde should be expanded in PATH dirs: {dir}"
        );
    }
}

#[test]
fn expand_tilde_works() {
    let expanded = expand_tilde("~/foo/bar");
    assert!(!expanded.starts_with("~"));
    assert!(expanded.ends_with("/foo/bar"));

    let absolute = expand_tilde("/usr/local/bin");
    assert_eq!(absolute, "/usr/local/bin");

    let just_tilde = expand_tilde("~");
    assert!(!just_tilde.starts_with("~"));
    assert!(!just_tilde.is_empty());
}

#[test]
fn no_bash_fallback_when_no_command() {
    // With no provider and no CLI command, should error not fallback to bash
    let config = LaunchConfig {
        providers: HashMap::new(),
        default_provider: "nonexistent".to_string(),
        ..Default::default()
    };
    let overrides = SessionLaunchOverrides {
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let err = resolve_from_config(&config, &overrides).unwrap_err();
    assert!(matches!(err, CreateSessionError::CommandEmpty));
}
