//! Behavior parity tests for session launch.
//!
//! Proves that both Tauri-style and Iced-style callers produce equivalent
//! backend launch requests through the same shared prepare_session function.

use std::collections::HashMap;
use std::path::PathBuf;

use planeai_core::session_launch::{
    prepare_session, CreateSessionError, CreateSessionRequest, SessionTarget,
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
