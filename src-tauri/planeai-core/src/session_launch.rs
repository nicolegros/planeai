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
    Rmux,
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
    /// Arguments that connect the agent to PlaneAI's MCP server; `{mcp_url}` is replaced
    /// with the server URL and the token is read from `PLANEAI_MCP_TOKEN`.
    #[serde(default)]
    pub mcp_args: Option<String>,
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

/// A provider PlaneAI configures out of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinProvider {
    pub key: &'static str,
    pub command: &'static str,
    pub yolo_flag: &'static str,
    pub resume_command: &'static str,
    /// Starts an interactive session on the prompt. `--` ends option parsing, so a prompt
    /// starting with `-` stays a prompt.
    pub prompt_command: &'static str,
    /// Empty when the agent has no verified per-launch way to add an MCP server.
    pub mcp_args: &'static str,
}

// `/bin/sh -c` keeps the single-quoted JSON intact; `cmd /C` has no single quotes, so on
// Windows the JSON is one double-quoted argument with escaped inner quotes. Claude itself
// expands `${PLANEAI_MCP_TOKEN}`, so the token never reaches a command line.
const CLAUDE_MCP_ARGS: &str = if cfg!(windows) {
    r#"--mcp-config "{\"mcpServers\":{\"planeai\":{\"type\":\"http\",\"url\":\"{mcp_url}\",\"headers\":{\"Authorization\":\"Bearer ${PLANEAI_MCP_TOKEN}\"}}}}" --allowedTools {mcp_allowed_tools}"#
} else {
    r#"--mcp-config '{"mcpServers":{"planeai":{"type":"http","url":"{mcp_url}","headers":{"Authorization":"Bearer ${PLANEAI_MCP_TOKEN}"}}}}' --allowedTools {mcp_allowed_tools}"#
};

// Codex reads `-c` values as TOML and falls back to a plain string, so the quotes are
// optional for both shells.
// Codex asks by default, so a tool the server starts serving after the launch asks too.
const CODEX_MCP_ARGS: &str = r#"-c mcp_servers.planeai.url="{mcp_url}" -c mcp_servers.planeai.bearer_token_env_var="PLANEAI_MCP_TOKEN" -c mcp_servers.planeai.default_tools_approval_mode="prompt" {mcp_codex_approved_tools}"#;

/// The Codex template v1.113 to v1.114 shipped, which approved every tool by default and
/// prompted through `{mcp_codex_prompted_tools}`; configs may still hold it.
const V1_114_CODEX_MCP_ARGS: &str = r#"-c mcp_servers.planeai.url="{mcp_url}" -c mcp_servers.planeai.bearer_token_env_var="PLANEAI_MCP_TOKEN" -c mcp_servers.planeai.default_tools_approval_mode="approve" {mcp_codex_prompted_tools}"#;
const V1_114_CODEX_PLACEHOLDER: &str = "{mcp_codex_prompted_tools}";
const V1_114_CODEX_DEFAULT: &str = r#"mcp_servers.planeai.default_tools_approval_mode="approve""#;

/// `mcp_args` with the v1.114 Codex approval scheme replaced by the current one: asking by
/// default and approving the listed tools. `Ok(None)` when there is nothing to upgrade, and
/// an error for a template whose approve-by-default PlaneAI cannot find to switch, which
/// must not launch: renaming its placeholder alone would approve every unlisted tool.
pub fn upgrade_mcp_args(args: &str) -> Result<Option<String>, String> {
    if args == V1_114_CODEX_MCP_ARGS {
        return Ok(Some(CODEX_MCP_ARGS.to_string()));
    }
    if !args.contains(V1_114_CODEX_PLACEHOLDER) {
        return Ok(None);
    }
    if !args.contains(V1_114_CODEX_DEFAULT) {
        return Err(format!(
            "mcp_args uses {V1_114_CODEX_PLACEHOLDER} without {V1_114_CODEX_DEFAULT}; replace them with the current Codex template"
        ));
    }
    Ok(Some(
        args.replace(
            V1_114_CODEX_DEFAULT,
            r#"mcp_servers.planeai.default_tools_approval_mode="prompt""#,
        )
        .replace(V1_114_CODEX_PLACEHOLDER, "{mcp_codex_approved_tools}"),
    ))
}

pub const BUILTIN_PROVIDERS: [BuiltinProvider; 4] = [
    BuiltinProvider {
        key: "kiro",
        command: "kiro-cli chat",
        yolo_flag: "--trust-all-tools",
        resume_command: "kiro-cli chat --resume",
        prompt_command: "-- {prompt}",
        mcp_args: "",
    },
    BuiltinProvider {
        key: "claude",
        command: "claude",
        yolo_flag: "--dangerously-skip-permissions",
        resume_command: "claude --resume",
        prompt_command: "-- {prompt}",
        mcp_args: CLAUDE_MCP_ARGS,
    },
    BuiltinProvider {
        key: "copilot",
        command: "copilot",
        yolo_flag: "--allow-all-tools",
        resume_command: "copilot --continue",
        // `-i` reads a prompt starting with `-` as a flag; the `=` form never does.
        prompt_command: "--interactive={prompt}",
        mcp_args: "",
    },
    BuiltinProvider {
        key: "codex",
        command: "codex",
        yolo_flag: "--dangerously-bypass-approvals-and-sandbox",
        resume_command: "codex resume --last",
        prompt_command: "-- {prompt}",
        mcp_args: CODEX_MCP_ARGS,
    },
];

impl Default for LaunchConfig {
    fn default() -> Self {
        let providers = BUILTIN_PROVIDERS
            .iter()
            .map(|builtin| {
                (
                    builtin.key.to_string(),
                    ProviderConfig {
                        command: builtin.command.to_string(),
                        yolo_flag: Some(builtin.yolo_flag.to_string()),
                        prompt_command: Some(builtin.prompt_command.to_string()),
                        mcp_args: Some(builtin.mcp_args.to_string()),
                    },
                )
            })
            .collect();
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
    let user_val: serde_json::Value = crate::jsonc::parse(&content)
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
    pub autonomous: bool,
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

/// Characters a task prompt may have: it travels as one command-line argument, and the
/// system bounds a command line (macOS `ARG_MAX` is 1 MiB) with the environment included.
pub const MAX_TASK_PROMPT_CHARS: usize = 100_000;

/// Refuse a task prompt no agent command line can carry, before anything is created for it.
pub fn check_task_prompt(prompt: &str) -> Result<(), String> {
    if prompt.contains('\0') {
        return Err("The task prompt contains a NUL character.".to_string());
    }
    if prompt.chars().count() > MAX_TASK_PROMPT_CHARS {
        return Err(format!(
            "The task prompt is longer than {MAX_TASK_PROMPT_CHARS} characters."
        ));
    }
    Ok(())
}

/// Build the provider launch command from provider config + launch parameters.
///
/// This is Layer A (provider/task command assembly), separate from Layer B
/// (session preparation: env, PATH, cwd, session id).
///
/// `autonomous` controls whether `autonomous_prompt_template` is applied.
/// Manual task launches should pass `false`; auto-dispatched launches pass `true`.
/// `mcp` is the MCP server the agent connects to, `None` when the server is disabled.
pub fn build_provider_launch_command(
    provider: &ProviderConfig,
    auto_approve: bool,
    task_prompt: Option<&str>,
    autonomous: bool,
    autonomous_prompt_template: Option<&str>,
    mcp: Option<&crate::mcp::McpLaunch>,
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

    // Before the prompt, which follows `--`.
    if let Some(args) = agent_launch_args(&provider.command, provider.mcp_args.as_deref(), mcp) {
        cmd = format!("{cmd} {args}");
    }

    // Inject task prompt via prompt_command if both are present
    let mut prompt_was_injected = false;
    if let (Some(prompt), Some(ref prompt_cmd)) = (task_prompt, &provider.prompt_command) {
        if !prompt.is_empty() {
            // Apply autonomous_prompt_template only for autonomous launches
            let final_prompt = if autonomous {
                if let Some(wrapper) = autonomous_prompt_template {
                    let mut vars = std::collections::HashMap::new();
                    vars.insert("prompt", prompt);
                    crate::template::render(wrapper, &vars)
                } else {
                    prompt.to_string()
                }
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

/// What PlaneAI adds to an agent's launch or resume command: Claude's hooks, then the MCP
/// arguments; `None` when there is nothing to add.
pub fn agent_launch_args(
    command: &str,
    mcp_args: Option<&str>,
    mcp: Option<&crate::mcp::McpLaunch>,
) -> Option<String> {
    let hooks = (crate::agent_hooks::AgentKind::from_command(command)
        == Some(crate::agent_hooks::AgentKind::Claude))
    .then(crate::agent_hooks::installed_claude_launch_args);
    let args: Vec<String> = hooks
        .into_iter()
        .chain(mcp_launch_args(mcp_args, mcp))
        .collect();
    (!args.is_empty()).then(|| args.join(" "))
}

/// A provider's `mcp_args` with its placeholders filled in, or `None` when the server is
/// not listening or the provider has no arguments for it. `{mcp_url}` is the server URL,
/// and `{mcp_allowed_tools}` (Claude Code's tool names) and `{mcp_codex_approved_tools}`
/// (Codex's per-tool overrides) list the tools that run without asking; every other tool
/// asks.
pub fn mcp_launch_args(
    template: Option<&str>,
    mcp: Option<&crate::mcp::McpLaunch>,
) -> Option<String> {
    use crate::mcp::SERVER_NAME;
    let (template, mcp) = (template?.trim(), mcp?);
    if template.is_empty() {
        return None;
    }
    // Settings saves and the Jira import publish a config that never went through its load.
    let upgraded = upgrade_mcp_args(template)
        .inspect_err(|error| tracing::warn!(%error, "agent launched without PlaneAI tools"))
        .ok()?;
    let template = upgraded.as_deref().unwrap_or(template);
    let allowed: Vec<String> = mcp
        .approved_tools()
        .map(|tool| format!("mcp__{SERVER_NAME}__{tool}"))
        .collect();
    let approved: Vec<String> = mcp
        .approved_tools()
        .map(|tool| format!(r#"-c mcp_servers.{SERVER_NAME}.tools.{tool}.approval_mode="approve""#))
        .collect();
    // Not `template::render`: it would also blank `${PLANEAI_MCP_TOKEN}`.
    Some(
        template
            .replace("{mcp_url}", &mcp.url)
            .replace("{mcp_allowed_tools}", &allowed.join(","))
            .replace("{mcp_codex_approved_tools}", &approved.join(" "))
            .trim()
            .to_string(),
    )
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
                overrides.autonomous,
                None, // autonomous_prompt_template not available in CLI context
                None,
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

    // Session target: CLI > config > default (daemon).
    //
    // Every known backend is named explicitly. Falling through to `Daemon` for a
    // configured-but-unlisted backend is how `rmux` was silently launched on the
    // daemon: the string is matched here, but `planeai-core` cannot see
    // `planeai_rmux::BACKEND`, so a new backend has to be added in both places.
    let config_target = match config.session_backend.as_deref() {
        Some("tmux") => SessionTarget::Tmux,
        Some("local") => SessionTarget::Local,
        Some("rmux") => SessionTarget::Rmux,
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

    /// A config that only names a backend; provider defaults supply the command.
    fn config_for_backend(backend: Option<&str>) -> LaunchConfig {
        let mut providers = HashMap::new();
        providers.insert(
            "kiro".to_string(),
            ProviderConfig {
                command: "kiro-cli chat".to_string(),
                yolo_flag: None,
                prompt_command: None,
                mcp_args: None,
            },
        );
        LaunchConfig {
            providers,
            default_provider: "kiro".to_string(),
            session_backend: backend.map(str::to_string),
            session_log_dir: None,
            extra_path_dirs: Vec::new(),
        }
    }

    fn resolved_target(backend: Option<&str>) -> SessionTarget {
        let overrides = SessionLaunchOverrides {
            cwd: Some(env::temp_dir()),
            ..Default::default()
        };
        resolve_from_config(&config_for_backend(backend), &overrides)
            .expect("resolution should succeed")
            .request
            .session_target
    }

    #[test]
    fn every_configured_backend_resolves_to_its_own_target() {
        // `rmux` previously fell through to `Daemon`, which launches an agent on
        // the wrong backend without reporting anything. Each name must map to
        // itself, so adding a backend to the config without adding it here fails.
        assert_eq!(resolved_target(Some("tmux")), SessionTarget::Tmux);
        assert_eq!(resolved_target(Some("local")), SessionTarget::Local);
        assert_eq!(resolved_target(Some("rmux")), SessionTarget::Rmux);
        assert_eq!(resolved_target(Some("daemon")), SessionTarget::Daemon);
    }

    #[test]
    fn an_absent_backend_still_defaults_to_the_daemon() {
        assert_eq!(resolved_target(None), SessionTarget::Daemon);
    }

    #[test]
    fn an_explicit_override_outranks_the_configured_backend() {
        let overrides = SessionLaunchOverrides {
            cwd: Some(env::temp_dir()),
            session_target: Some(SessionTarget::Rmux),
            ..Default::default()
        };
        let resolved = resolve_from_config(&config_for_backend(Some("tmux")), &overrides)
            .expect("resolution should succeed");
        assert_eq!(resolved.request.session_target, SessionTarget::Rmux);
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

    fn builtin(key: &str) -> ProviderConfig {
        LaunchConfig::default().providers.remove(key).unwrap()
    }

    const MCP_URL: &str = "http://127.0.0.1:47437/mcp";

    fn server() -> crate::mcp::McpLaunch {
        crate::mcp::McpLaunch::new(MCP_URL)
    }

    #[test]
    fn mcp_args_fill_in_the_url_and_keep_the_token_reference_for_the_agent() {
        assert_eq!(
            mcp_launch_args(
                Some("--url {mcp_url} --token ${PLANEAI_MCP_TOKEN}"),
                Some(&server())
            ),
            Some(format!("--url {MCP_URL} --token ${{PLANEAI_MCP_TOKEN}}"))
        );
    }

    #[test]
    fn mcp_args_are_skipped_when_the_server_is_off_or_the_template_is_empty() {
        assert_eq!(mcp_launch_args(Some("--url {mcp_url}"), None), None);
        assert_eq!(mcp_launch_args(Some("  "), Some(&server())), None);
        assert_eq!(mcp_launch_args(None, Some(&server())), None);
    }

    #[test]
    fn claude_launches_load_planeais_hooks_before_the_mcp_arguments() {
        let hooks = crate::agent_hooks::installed_claude_launch_args();
        assert_eq!(
            agent_launch_args(
                "/opt/homebrew/bin/claude --resume",
                Some("--mcp {mcp_url}"),
                Some(&server())
            ),
            Some(format!("{hooks} --mcp http://127.0.0.1:47437/mcp"))
        );
        assert_eq!(agent_launch_args("claude", None, None), Some(hooks));
        assert_eq!(agent_launch_args("codex", None, None), None);
        assert_eq!(
            agent_launch_args(
                "my-claude-wrapper",
                Some("--mcp {mcp_url}"),
                Some(&server())
            ),
            Some("--mcp http://127.0.0.1:47437/mcp".into())
        );
    }

    #[test]
    fn mcp_args_go_after_the_yolo_flag_and_before_the_prompt() {
        let provider = ProviderConfig {
            command: "agent".into(),
            yolo_flag: Some("--yolo".into()),
            prompt_command: Some("-- {prompt}".into()),
            mcp_args: Some("--mcp {mcp_url}".into()),
        };
        let launch = build_provider_launch_command(
            &provider,
            true,
            Some("hi"),
            false,
            None,
            Some(&server()),
        );
        assert_eq!(
            launch.command,
            format!("agent --yolo --mcp {MCP_URL} -- 'hi'")
        );
        let off = build_provider_launch_command(&provider, true, Some("hi"), false, None, None);
        assert_eq!(off.command, "agent --yolo -- 'hi'");
    }

    #[test]
    fn the_approval_placeholders_list_every_tool_but_deletes() {
        let mut server = server();
        server.approved_plugin_tools = vec!["local-fixture_echo".into()];
        let names = [
            "task_list",
            "task_show",
            "task_create",
            "task_update",
            "session_list",
            "session_tree",
            "session_read",
            "session_create",
            "session_prompt",
            "project_list",
            "local-fixture_echo",
        ];
        assert_eq!(
            mcp_launch_args(Some("--allow {mcp_allowed_tools}"), Some(&server)).unwrap(),
            format!(
                "--allow {}",
                names.map(|name| format!("mcp__planeai__{name}")).join(",")
            )
        );
        assert_eq!(
            mcp_launch_args(Some("{mcp_codex_approved_tools}"), Some(&server)).unwrap(),
            names
                .map(|name| format!(
                    "-c mcp_servers.planeai.tools.{name}.approval_mode=\"approve\""
                ))
                .join(" ")
        );
    }

    #[test]
    fn a_v1_114_codex_template_launches_with_the_current_approvals() {
        let codex = builtin("codex").mcp_args.unwrap();
        assert_eq!(
            upgrade_mcp_args(V1_114_CODEX_MCP_ARGS),
            Ok(Some(codex.clone()))
        );
        assert_eq!(upgrade_mcp_args(&codex), Ok(None));
        assert_eq!(
            mcp_launch_args(Some(V1_114_CODEX_MCP_ARGS), Some(&server())),
            mcp_launch_args(Some(&codex), Some(&server()))
        );
        let custom = r#"--profile x -c mcp_servers.planeai.default_tools_approval_mode="approve" {mcp_codex_prompted_tools}"#;
        let launched = mcp_launch_args(Some(custom), Some(&server())).unwrap();
        assert!(launched.starts_with(
            r#"--profile x -c mcp_servers.planeai.default_tools_approval_mode="prompt" -c mcp_servers.planeai.tools.task_list.approval_mode="approve""#
        ));
        assert!(!launched.contains('{'));
    }

    #[test]
    fn a_v1_114_template_whose_default_cannot_be_switched_launches_without_tools() {
        for custom in [
            "-c mcp_servers.planeai.default_tools_approval_mode='approve' {mcp_codex_prompted_tools}",
            "-c mcp_servers.planeai.default_tools_approval_mode=approve {mcp_codex_prompted_tools}",
            "{mcp_codex_prompted_tools}",
        ] {
            assert!(upgrade_mcp_args(custom).is_err(), "{custom}");
            assert_eq!(mcp_launch_args(Some(custom), Some(&server())), None, "{custom}");
        }
    }

    #[test]
    fn the_upgrade_switches_only_planeais_default() {
        let custom = r#"-c mcp_servers.other.default_tools_approval_mode="approve" -c mcp_servers.planeai.default_tools_approval_mode="approve" {mcp_codex_prompted_tools}"#;
        assert_eq!(
            upgrade_mcp_args(custom),
            Ok(Some(
                r#"-c mcp_servers.other.default_tools_approval_mode="approve" -c mcp_servers.planeai.default_tools_approval_mode="prompt" {mcp_codex_approved_tools}"#
                    .to_string()
            ))
        );
    }

    #[test]
    fn the_built_in_templates_leave_tool_approval_to_the_placeholders() {
        assert!(builtin("claude")
            .mcp_args
            .unwrap()
            .ends_with("--allowedTools {mcp_allowed_tools}"));
        assert!(builtin("codex")
            .mcp_args
            .unwrap()
            .ends_with(r#"default_tools_approval_mode="prompt" {mcp_codex_approved_tools}"#));
    }

    #[test]
    fn only_claude_and_codex_ship_mcp_args() {
        assert_eq!(builtin("kiro").mcp_args.as_deref(), Some(""));
        assert_eq!(builtin("copilot").mcp_args.as_deref(), Some(""));
        assert!(builtin("claude").mcp_args.unwrap().contains("{mcp_url}"));
        assert!(builtin("codex").mcp_args.unwrap().contains("{mcp_url}"));
    }

    /// Runs the built command the way sessions do, with the agent replaced by `printf`.
    #[cfg(not(windows))]
    fn shell_argv(provider_key: &str, prompt: Option<&str>) -> Vec<String> {
        let mut provider = builtin(provider_key);
        provider.command = "printf '%s\\n'".into();
        let cmd =
            build_provider_launch_command(&provider, false, prompt, false, None, Some(&server()))
                .command;
        let (program, args) = shell_args(&cmd);
        let output = std::process::Command::new(program)
            .args(args)
            .env("PLANEAI_MCP_TOKEN", "secret-token")
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[cfg(not(windows))]
    #[test]
    fn claude_receives_its_mcp_config_as_one_json_argument_through_the_shell() {
        let argv = shell_argv("claude", Some("fix it"));
        assert_eq!(argv[0], "--mcp-config");
        let config: serde_json::Value = serde_json::from_str(&argv[1]).unwrap();
        assert_eq!(
            config,
            serde_json::json!({"mcpServers": {"planeai": {
                "type": "http",
                "url": MCP_URL,
                "headers": {"Authorization": "Bearer ${PLANEAI_MCP_TOKEN}"}
            }}})
        );
        assert_eq!(argv[2], "--allowedTools");
        assert_eq!(
            argv[3].split(',').collect::<Vec<_>>(),
            [
                "mcp__planeai__task_list",
                "mcp__planeai__task_show",
                "mcp__planeai__task_create",
                "mcp__planeai__task_update",
                "mcp__planeai__session_list",
                "mcp__planeai__session_tree",
                "mcp__planeai__session_read",
                "mcp__planeai__session_create",
                "mcp__planeai__session_prompt",
                "mcp__planeai__project_list",
            ]
        );
        assert_eq!(argv[4..], ["--", "fix it"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn codex_receives_its_mcp_server_as_config_overrides_through_the_shell() {
        let argv = shell_argv("codex", None);
        assert_eq!(
            argv[..6],
            [
                "-c",
                "mcp_servers.planeai.url=http://127.0.0.1:47437/mcp",
                "-c",
                "mcp_servers.planeai.bearer_token_env_var=PLANEAI_MCP_TOKEN",
                "-c",
                "mcp_servers.planeai.default_tools_approval_mode=prompt",
            ]
        );
        assert!(argv.contains(&"mcp_servers.planeai.tools.task_list.approval_mode=approve".into()));
        assert!(!argv.iter().any(|arg| arg.contains("task_delete")));
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
