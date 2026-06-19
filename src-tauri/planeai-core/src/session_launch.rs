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

// ─── Shared config types (UI-neutral) ────────────────────────────────────────

/// Minimal provider definition needed for session launch.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ProviderConfig {
    pub command: String,
    #[serde(default)]
    pub yolo_flag: Option<String>,
    #[serde(default)]
    pub prompt_command: Option<String>,
    #[serde(default)]
    pub autonomous_prompt_template: Option<String>,
}

/// Minimal app config subset needed for session launch resolution.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct LaunchConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default)]
    pub session_backend: Option<String>,
    #[serde(default)]
    pub session_log_dir: Option<String>,
    #[serde(default)]
    pub extra_path_dirs: Vec<String>,
}

fn default_provider() -> String {
    "kiro".to_string()
}

impl Default for LaunchConfig {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "kiro".to_string(),
            ProviderConfig {
                command: "kiro-cli chat".to_string(),
                yolo_flag: Some("--trust-all-tools".to_string()),
                prompt_command: Some("{prompt}".to_string()),
                autonomous_prompt_template: None,
            },
        );
        providers.insert(
            "claude".to_string(),
            ProviderConfig {
                command: "claude".to_string(),
                yolo_flag: Some("--dangerously-skip-permissions".to_string()),
                prompt_command: Some("-p {prompt}".to_string()),
                autonomous_prompt_template: None,
            },
        );
        Self {
            providers,
            default_provider: "kiro".to_string(),
            session_backend: None,
            session_log_dir: None,
            extra_path_dirs: Vec::new(),
        }
    }
}

/// Expand `~` prefix to the user's home directory.
pub fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        format!("{home}/{rest}")
    } else if path == "~" {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default()
    } else {
        path.to_string()
    }
}

/// Load a LaunchConfig from a JSON/JSONC file, merged over defaults.
pub fn load_launch_config(path: &std::path::Path) -> Result<LaunchConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
    // Strip JSONC comments
    let stripped = json_comments::StripComments::new(content.as_bytes());
    let user_val: serde_json::Value = serde_json::from_reader(stripped)
        .map_err(|e| format!("cannot parse config {}: {e}", path.display()))?;
    // Merge user values over defaults
    let default_val = serde_json::to_value(LaunchConfig::default()).unwrap();
    let merged = merge_values(default_val, user_val);
    serde_json::from_value(merged)
        .map_err(|e| format!("cannot deserialize config {}: {e}", path.display()))
}

/// Shallow merge: user keys override default keys at the top level.
/// For objects, user fields replace default fields (not deep merge).
fn merge_values(default: serde_json::Value, user: serde_json::Value) -> serde_json::Value {
    match (default, user) {
        (serde_json::Value::Object(mut def), serde_json::Value::Object(usr)) => {
            for (k, v) in usr {
                def.insert(k, v);
            }
            serde_json::Value::Object(def)
        }
        (_, user) => user,
    }
}

/// Returns the platform-appropriate PlaneAI config directory.
pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| format!("{home}\\AppData\\Roaming"));
        PathBuf::from(base).join("planeai")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
        PathBuf::from(base).join("planeai")
    }
}

/// Load config from the standard PlaneAI config location (JSONC, merged over defaults).
pub fn load_default_config() -> LaunchConfig {
    let path = config_dir().join("config.json");
    if path.exists() {
        load_launch_config(&path).unwrap_or_default()
    } else {
        LaunchConfig::default()
    }
}

// ─── Overrides and resolver ──────────────────────────────────────────────────

/// CLI/env overrides for session launch.
#[derive(Debug, Clone, Default)]
pub struct SessionLaunchOverrides {
    pub cwd: Option<PathBuf>,
    pub agent_command: Option<String>,
    pub provider_id: Option<String>,
    pub extra_path_dirs: Vec<String>,
    pub session_target: Option<SessionTarget>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub auto_approve: bool,
    pub task_prompt: Option<String>,
}

/// Resolved launch configuration with provenance notes.
#[derive(Debug, Clone)]
pub struct ResolvedLaunchConfig {
    pub request: CreateSessionRequest,
    pub provider_label: Option<String>,
    pub command_label: String,
    pub session_log_dir: Option<String>,
    pub prompt_was_injected: bool,
    pub auto_approve_was_applied: bool,
}

/// Result of building a provider launch command (Layer A: provider/task command assembly).
#[derive(Debug, Clone)]
pub struct ProviderLaunchCommand {
    pub command: String,
    pub prompt_was_injected: bool,
    pub auto_approve_was_applied: bool,
}

/// Build the provider launch command from provider config + launch parameters.
///
/// This is Layer A (provider/task command assembly), separate from Layer B
/// (session preparation: env, PATH, cwd, session id).
pub fn build_provider_launch_command(
    provider: &ProviderConfig,
    auto_approve: bool,
    task_prompt: Option<&str>,
) -> ProviderLaunchCommand {
    let mut cmd = provider.command.clone();
    let mut auto_approve_was_applied = false;

    // Append yolo flag if auto-approve is requested
    if auto_approve {
        if let Some(ref flag) = provider.yolo_flag {
            cmd = format!("{cmd} {flag}");
            auto_approve_was_applied = true;
        }
    }

    // Inject task prompt via prompt_command if both are present
    let mut prompt_was_injected = false;
    if let (Some(prompt), Some(ref prompt_cmd)) = (task_prompt, &provider.prompt_command) {
        if !prompt.is_empty() {
            // Apply autonomous_prompt_template wrapper if configured
            let final_prompt = if let Some(ref wrapper) = provider.autonomous_prompt_template {
                let mut vars = std::collections::HashMap::new();
                vars.insert("prompt", prompt);
                crate::template::render(wrapper, &vars)
            } else {
                prompt.to_string()
            };
            crate::template::append_prompt(&mut cmd, prompt_cmd, &final_prompt);
            prompt_was_injected = true;
        }
    }

    ProviderLaunchCommand {
        command: cmd,
        prompt_was_injected,
        auto_approve_was_applied,
    }
}

/// Resolve a CreateSessionRequest from config + overrides.
///
/// Precedence: CLI overrides > env vars > config file > defaults.
pub fn resolve_from_config(
    config: &LaunchConfig,
    overrides: &SessionLaunchOverrides,
) -> Result<ResolvedLaunchConfig, CreateSessionError> {
    // Provider resolution
    let provider_id = overrides
        .provider_id
        .as_deref()
        .unwrap_or(&config.default_provider);
    let provider = config.providers.get(provider_id);

    // Command resolution: CLI > provider config > error
    let (agent_command, prompt_was_injected, auto_approve_was_applied) =
        if let Some(ref cmd) = overrides.agent_command {
            // Explicit CLI command — no provider assembly
            (cmd.clone(), false, false)
        } else if let Some(p) = provider {
            let result = build_provider_launch_command(
                p,
                overrides.auto_approve,
                overrides.task_prompt.as_deref(),
            );
            (
                result.command,
                result.prompt_was_injected,
                result.auto_approve_was_applied,
            )
        } else {
            return Err(CreateSessionError::CommandEmpty);
        };

    // CWD: CLI > current dir
    let cwd = overrides
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Extra PATH dirs: config + CLI, all tilde-expanded
    let mut extra_path_dirs: Vec<String> = config
        .extra_path_dirs
        .iter()
        .chain(overrides.extra_path_dirs.iter())
        .map(|d| expand_tilde(d))
        .collect();
    // Deduplicate
    let mut seen = std::collections::HashSet::new();
    extra_path_dirs.retain(|d| seen.insert(d.clone()));

    // Session target: CLI > config > default (daemon)
    let config_target = match config.session_backend.as_deref() {
        Some("tmux") => SessionTarget::Tmux,
        Some("local") => SessionTarget::Local,
        _ => SessionTarget::Daemon,
    };
    let session_target = overrides.session_target.clone().unwrap_or(config_target);

    // Session log dir: env > config
    let session_log_dir = std::env::var("PLANEAI_SESSION_LOG_DIR")
        .ok()
        .or_else(|| config.session_log_dir.as_ref().map(|d| expand_tilde(d)));
    let durable_logs = session_log_dir.is_some();

    let session_id = uuid::Uuid::new_v4().to_string();

    let request = CreateSessionRequest {
        session_id,
        project_cwd: cwd,
        session_target,
        agent_command: agent_command.clone(),
        env: HashMap::new(),
        extra_path_dirs,
        cols: overrides.cols.unwrap_or(120),
        rows: overrides.rows.unwrap_or(40),
        durable_logs,
    };

    Ok(ResolvedLaunchConfig {
        request,
        provider_label: Some(provider_id.to_string()),
        command_label: agent_command,
        session_log_dir,
        prompt_was_injected,
        auto_approve_was_applied,
    })
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
