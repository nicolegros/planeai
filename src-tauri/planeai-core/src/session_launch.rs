use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use crate::command::{augmented_path, shell_args};

/// Target backend for session launch.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionTarget {
    Local,
    Daemon,
    Tmux,
}

/// UI-neutral request to create a session.
#[derive(Debug, Clone)]
pub struct CreateSessionRequest {
    pub session_id: String,
    pub project_cwd: PathBuf,
    pub session_target: SessionTarget,
    pub agent_command: String,
    pub env: HashMap<String, String>,
    pub extra_path_dirs: Vec<String>,
    pub cols: u16,
    pub rows: u16,
    pub durable_logs: bool,
}

/// Result of a successful session launch.
#[derive(Debug, Clone)]
pub struct CreateSessionResult {
    pub session_id: String,
    pub target: SessionTarget,
    pub command_label: String,
    pub cwd: PathBuf,
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

#[derive(Debug)]
pub enum CreateSessionError {
    InvalidCwd(String),
    CommandEmpty,
    UnsupportedTarget(String),
}

impl fmt::Display for CreateSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCwd(path) => write!(f, "invalid cwd: {path}"),
            Self::CommandEmpty => write!(f, "agent command is empty"),
            Self::UnsupportedTarget(t) => write!(f, "unsupported target: {t}"),
        }
    }
}

impl std::error::Error for CreateSessionError {}

/// Resolve and validate a session launch request without spawning anything.
pub fn prepare_session(
    req: &CreateSessionRequest,
) -> Result<CreateSessionResult, CreateSessionError> {
    if !req.project_cwd.is_dir() {
        return Err(CreateSessionError::InvalidCwd(
            req.project_cwd.display().to_string(),
        ));
    }

    let cmd = req.agent_command.trim();
    if cmd.is_empty() {
        return Err(CreateSessionError::CommandEmpty);
    }

    let (program, args) = shell_args(cmd);

    let mut env = req.env.clone();
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env.insert("PLANEAI_SESSION_ID".to_string(), req.session_id.clone());
    env.insert("PATH".to_string(), augmented_path(&req.extra_path_dirs));

    Ok(CreateSessionResult {
        session_id: req.session_id.clone(),
        target: req.session_target.clone(),
        command_label: cmd.to_string(),
        cwd: req.project_cwd.clone(),
        program: program.to_string(),
        args,
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn valid_request() -> CreateSessionRequest {
        CreateSessionRequest {
            session_id: "test-123".to_string(),
            project_cwd: env::temp_dir(),
            session_target: SessionTarget::Daemon,
            agent_command: "kiro-cli chat".to_string(),
            env: HashMap::new(),
            extra_path_dirs: vec!["/custom/bin".to_string()],
            cols: 80,
            rows: 24,
            durable_logs: false,
        }
    }

    #[test]
    fn prepare_session_valid_inputs() {
        let req = valid_request();
        let result = prepare_session(&req).unwrap();
        assert_eq!(result.session_id, "test-123");
        assert_eq!(result.target, SessionTarget::Daemon);
        assert_eq!(result.command_label, "kiro-cli chat");
        assert_eq!(result.cwd, env::temp_dir());
        assert!(!result.program.is_empty());
        assert!(!result.args.is_empty());
    }

    #[test]
    fn prepare_session_nonexistent_cwd() {
        let mut req = valid_request();
        req.project_cwd = PathBuf::from("/nonexistent/path/xyz");
        let err = prepare_session(&req).unwrap_err();
        assert!(matches!(err, CreateSessionError::InvalidCwd(_)));
    }

    #[test]
    fn prepare_session_empty_command() {
        let mut req = valid_request();
        req.agent_command = "   ".to_string();
        let err = prepare_session(&req).unwrap_err();
        assert!(matches!(err, CreateSessionError::CommandEmpty));
    }

    #[test]
    fn env_contains_required_vars() {
        let result = prepare_session(&valid_request()).unwrap();
        assert_eq!(result.env.get("TERM").unwrap(), "xterm-256color");
        assert_eq!(result.env.get("PLANEAI_SESSION_ID").unwrap(), "test-123");
        assert!(result.env.contains_key("PATH"));
        assert!(result.env["PATH"].contains("/custom/bin"));
    }

    #[test]
    fn uses_shell_args() {
        let result = prepare_session(&valid_request()).unwrap();
        #[cfg(not(windows))]
        {
            assert_eq!(result.program, "/bin/sh");
            assert_eq!(result.args[0], "-c");
            assert_eq!(result.args[1], "kiro-cli chat");
        }
        #[cfg(windows)]
        {
            assert_eq!(result.program, "cmd");
            assert_eq!(result.args[0], "/C");
        }
    }
}
