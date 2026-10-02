//! Notification hooks installed into each supported agent CLI's own config.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A CLI coding agent planeai knows how to install notification hooks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Kiro,
    Claude,
    Copilot,
    Codex,
}

impl AgentKind {
    pub const ALL: [AgentKind; 4] = [
        AgentKind::Kiro,
        AgentKind::Claude,
        AgentKind::Copilot,
        AgentKind::Codex,
    ];

    fn binary_name(self) -> &'static str {
        match self {
            AgentKind::Kiro => "kiro",
            AgentKind::Claude => "claude",
            AgentKind::Copilot => "copilot",
            AgentKind::Codex => "codex",
        }
    }

    /// Identify the agent a provider command launches, e.g. `kiro-cli chat`,
    /// `/opt/homebrew/bin/claude --resume` or `npx copilot`.
    ///
    /// Matches a command word whose file name is the agent name or starts with
    /// `<name>-`, so wrappers like `my-claude-wrapper` are not misdetected.
    pub fn from_command(command: &str) -> Option<Self> {
        agent_word(command).map(|(kind, _)| kind)
    }

    pub fn is_hook_installed(self, home: &str) -> bool {
        let home = Path::new(home);
        match self {
            AgentKind::Kiro => {
                is_kiro_hook_installed_at(&home.join(".kiro/agents/default.json"))
                    || is_kiro_v3_hook_installed_at(&home.join(".kiro/hooks"))
            }
            AgentKind::Claude => is_claude_hook_installed_at(&home.join(".claude/settings.json")),
            AgentKind::Copilot => is_copilot_hook_installed_at(&copilot_dir(home)),
            AgentKind::Codex => is_codex_hook_installed_at(&codex_dir(home)),
        }
    }

    /// Install (or repair) the hook script and register it in the agent's config.
    /// Idempotent: existing user config is preserved and planeai entries are never duplicated.
    pub fn install_hook(self, home: &str) -> Result<(), String> {
        match self {
            AgentKind::Kiro => install_kiro_hook(home),
            AgentKind::Claude => install_claude_hook(home),
            AgentKind::Copilot => install_copilot_hook(home),
            AgentKind::Codex => install_codex_hook(home),
        }
    }
}

/// The agent a command launches, with the byte offset where its executable word ends.
fn agent_word(command: &str) -> Option<(AgentKind, usize)> {
    command.split_whitespace().find_map(|word| {
        let file_name = word
            .trim_matches(|c| c == '"' || c == '\'')
            .rsplit(['/', '\\'])
            .next()?;
        let name = file_name.strip_suffix(".exe").unwrap_or(file_name);
        let kind = AgentKind::ALL.into_iter().find(|kind| {
            let bin = kind.binary_name();
            name == bin
                || name
                    .strip_prefix(bin)
                    .is_some_and(|rest| rest.starts_with('-'))
        })?;
        let start = word.as_ptr() as usize - command.as_ptr() as usize;
        Some((kind, start + word.len()))
    })
}

/// Adapt a provider command so the agent's hook events are attributed to its own session.
///
/// Codex runs hooks in a shared app-server daemon that keeps the environment of whichever
/// session started it, so every session would report that one's `PLANEAI_SESSION_ID`.
pub fn session_scoped_command(command: &str) -> String {
    codex_without_daemon(command, codex_supports_no_daemon)
}

fn codex_without_daemon(command: &str, supports_no_daemon: impl FnOnce(&str) -> bool) -> String {
    let Some((AgentKind::Codex, end)) = agent_word(command) else {
        return command.to_string();
    };
    let daemon_chosen = command
        .split_whitespace()
        .any(|w| w == "--no-daemon" || w == "--remote" || w.starts_with("--remote="));
    if daemon_chosen || !supports_no_daemon(&command[..end]) {
        return command.to_string();
    }
    format!("{} --no-daemon{}", &command[..end], &command[end..])
}

/// Older Codex releases reject unknown flags, so check `--help` once per launcher.
fn codex_supports_no_daemon(launcher: &str) -> bool {
    static SUPPORT: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    if let Some(&known) = SUPPORT
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .get(launcher)
    {
        return known;
    }
    let supported = help_mentions(launcher, "--no-daemon");
    SUPPORT
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(launcher.to_string(), supported);
    supported
}

fn help_mentions(launcher: &str, flag: &str) -> bool {
    let (program, args) = crate::command::shell_args(&format!("{launcher} --help"));
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .env("PATH", crate::command::augmented_path(&[]))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::command::no_window(&mut cmd);
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    // Help text fits in the pipe buffer, so waiting before reading cannot deadlock.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
    let mut help = String::new();
    child
        .stdout
        .take()
        .is_some_and(|mut out| out.read_to_string(&mut help).is_ok())
        && help.contains(flag)
}

/// Rewrite already-installed hooks so their scripts match the bundled version.
/// Agents whose hook is not installed are left untouched.
pub fn refresh_hook_scripts(home: &str) {
    for kind in AgentKind::ALL {
        if kind.is_hook_installed(home) {
            if let Err(e) = kind.install_hook(home) {
                tracing::warn!(?kind, error = %e, "failed to refresh hook script");
            }
        }
    }
}

fn copilot_dir(home: &Path) -> PathBuf {
    std::env::var("COPILOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".copilot"))
}

fn codex_dir(home: &Path) -> PathBuf {
    std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".codex"))
}

const CODEX_HOOK_MARKER: &str = "planeai-stop-notify-codex";

/// Codex hook events and the argument passed to the notify script for each.
/// `PostToolUse` flips the session back to busy after a permission prompt is approved.
/// `PermissionRequest` is deliberately absent: with `approvals_reviewer = "auto_review"`
/// it fires for every escalated command even though no human is needed.
const CODEX_HOOK_EVENTS: [(&str, &str); 3] = [
    ("UserPromptSubmit", "busy"),
    ("PostToolUse", "busy"),
    ("Stop", "stop"),
];

// ─── Hook detection ──────────────────────────────────────────────────────────

/// Case-insensitive lookup in a JSON object for a key.
fn get_ignore_case<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a serde_json::Value> {
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Check if a v3 hooks array contains a trigger with the given name (case-insensitive)
/// whose action command contains the planeai notify marker.
fn has_v3_notify_trigger(hooks: &[serde_json::Value], trigger_name: &str) -> bool {
    hooks.iter().any(|h| {
        let trigger = h.get("trigger").and_then(|t| t.as_str()).unwrap_or("");
        let cmd = h
            .get("action")
            .and_then(|a| a.get("command"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        trigger.eq_ignore_ascii_case(trigger_name) && cmd.contains("planeai-stop-notify")
    })
}

pub fn is_kiro_hook_installed_at(config_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return false;
    };
    if !content.contains("planeai-stop-notify") {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(hooks) = v.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    let has_stop = get_ignore_case(hooks, "stop")
        .and_then(|a| a.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("planeai-stop-notify"))
            })
        });
    let has_prompt = get_ignore_case(hooks, "userPromptSubmit")
        .and_then(|a| a.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("planeai-stop-notify"))
            })
        });
    has_stop && has_prompt
}

/// Check if the planeai notification hook is installed in the Kiro CLI v3 hooks directory.
///
/// v3 hooks live in `.kiro/hooks/<name>.json` with schema:
/// ```json
/// { "version": "v1", "hooks": [{ "trigger": "Stop", "action": { "type": "command", "command": "..." } }] }
/// ```
pub fn is_kiro_v3_hook_installed_at(hooks_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(hooks_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !content.contains("planeai-stop-notify") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(hooks) = v.get("hooks").and_then(|h| h.as_array()) else {
            continue;
        };
        if has_v3_notify_trigger(hooks, "Stop") && has_v3_notify_trigger(hooks, "UserPromptSubmit")
        {
            return true;
        }
    }
    false
}

pub fn is_claude_hook_installed_at(settings_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(settings_path) else {
        return false;
    };
    if !content.contains("planeai-stop-notify") {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(hooks) = v.get("hooks").and_then(|h| h.as_object()) else {
        return false;
    };
    ["Stop", "StopFailure", "Notification", "UserPromptSubmit"]
        .iter()
        .all(|event| hooks.contains_key(*event))
}

pub fn is_copilot_hook_installed_at(copilot_dir: &Path) -> bool {
    let notify_path = copilot_dir.join("hooks").join("planeai-notify.json");
    let Ok(content) = std::fs::read_to_string(notify_path) else {
        return false;
    };
    content.contains("planeai-stop-notify-copilot")
}

fn codex_group_has_marker(group: &serde_json::Value) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains(CODEX_HOOK_MARKER))
            })
        })
}

pub fn is_codex_hook_installed_at(codex_dir: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(codex_dir.join("hooks.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    CODEX_HOOK_EVENTS.iter().all(|(event, _)| {
        v.pointer(&format!("/hooks/{event}"))
            .and_then(|groups| groups.as_array())
            .is_some_and(|groups| groups.iter().any(codex_group_has_marker))
    })
}

// ─── Hook installation ───────────────────────────────────────────────────────

pub fn install_kiro_hook(home: &str) -> Result<(), String> {
    let hooks_dir = format!("{home}/.kiro/hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.sh"),
        include_str!("../resources/planeai-stop-notify.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.ps1"),
        include_str!("../resources/planeai-stop-notify.ps1"),
    );

    std::fs::write(&script_path, script_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    let agents_dir = format!("{home}/.kiro/agents");
    std::fs::create_dir_all(&agents_dir)
        .map_err(|e| format!("failed to create agents dir: {e}"))?;
    let config_path = format!("{agents_dir}/default.json");

    let mut config: serde_json::Value = if let Ok(content) = std::fs::read_to_string(&config_path) {
        serde_json::from_str(&content).map_err(|e| format!("failed to parse default.json: {e}"))?
    } else {
        serde_json::json!({ "name": "default", "tools": ["*"] })
    };

    #[cfg(not(windows))]
    let hook_command = format!("{hooks_dir}/planeai-stop-notify.sh");
    #[cfg(windows)]
    let hook_command =
        format!("powershell -NoProfile -File \"{hooks_dir}/planeai-stop-notify.ps1\"");

    let hooks = config
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().unwrap();

    let mut ensure_hook = |event: &str| {
        let arr = hooks_obj
            .entry(event)
            .or_insert_with(|| serde_json::json!([]));
        let arr = arr.as_array_mut().unwrap();
        let already = arr.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|c| c.contains("planeai-stop-notify"))
        });
        if !already {
            arr.push(serde_json::json!({ "command": hook_command }));
        }
    };

    ensure_hook("stop");
    ensure_hook("userPromptSubmit");

    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, output)
        .map_err(|e| format!("failed to write default.json: {e}"))?;
    Ok(())
}

pub fn install_claude_hook(home: &str) -> Result<(), String> {
    let claude_dir = std::path::PathBuf::from(format!("{home}/.claude"));
    let hooks_dir = claude_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.sh"),
        include_str!("../resources/planeai-stop-notify-claude.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.ps1"),
        include_str!("../resources/planeai-stop-notify-claude.ps1"),
    );

    std::fs::write(&script_path, script_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    let script_command = script_path.to_string_lossy().to_string();
    install_claude_hook_at(&claude_dir, &script_command)
}

pub fn install_copilot_hook(home: &str) -> Result<(), String> {
    let copilot_dir = copilot_dir(Path::new(home));
    let hooks_dir = copilot_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (bash_path, bash_content) = (
        hooks_dir.join("planeai-stop-notify-copilot.sh"),
        include_str!("../resources/planeai-stop-notify-copilot.sh"),
    );
    #[cfg(not(windows))]
    {
        std::fs::write(&bash_path, bash_content)
            .map_err(|e| format!("failed to write hook script: {e}"))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bash_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    #[cfg(windows)]
    let bash_path = hooks_dir.join("planeai-stop-notify-copilot.sh");

    let ps_path = hooks_dir.join("planeai-stop-notify-copilot.ps1");
    let ps_content = include_str!("../resources/planeai-stop-notify-copilot.ps1");
    std::fs::write(&ps_path, ps_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;

    install_copilot_hook_at(
        &copilot_dir,
        &bash_path.to_string_lossy(),
        &ps_path.to_string_lossy(),
    )
}

pub fn install_copilot_hook_at(
    copilot_dir: &Path,
    bash_script: &str,
    ps_script: &str,
) -> Result<(), String> {
    let hooks_dir = copilot_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    let config = serde_json::json!({
        "version": 1,
        "hooks": {
            "agentStop": [{ "type": "command", "bash": format!("{bash_script} stop"), "powershell": format!("{ps_script} stop"), "timeoutSec": 5 }],
            "userPromptSubmitted": [{ "type": "command", "bash": format!("{bash_script} busy"), "powershell": format!("{ps_script} busy"), "timeoutSec": 5 }],
            "errorOccurred": [{ "type": "command", "bash": format!("{bash_script} notification"), "powershell": format!("{ps_script} notification"), "timeoutSec": 5 }]
        }
    });

    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(hooks_dir.join("planeai-notify.json"), output)
        .map_err(|e| format!("failed to write planeai-notify.json: {e}"))?;
    Ok(())
}

pub fn install_claude_hook_at(claude_dir: &Path, script_command: &str) -> Result<(), String> {
    std::fs::create_dir_all(claude_dir)
        .map_err(|e| format!("failed to create .claude dir: {e}"))?;

    let settings_path = claude_dir.join("settings.json");
    let mut settings: serde_json::Value = if let Ok(content) =
        std::fs::read_to_string(&settings_path)
    {
        serde_json::from_str(&content).map_err(|e| format!("failed to parse settings.json: {e}"))?
    } else {
        serde_json::json!({})
    };

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().unwrap();

    let hook_entry = serde_json::json!({
        "type": "command",
        "command": script_command,
        "args": []
    });

    let mut ensure_hook = |event: &str, matcher: Option<&str>| {
        let arr = hooks_obj
            .entry(event)
            .or_insert_with(|| serde_json::json!([]));
        let arr = arr.as_array_mut().unwrap();
        if arr.iter().any(|g| {
            serde_json::to_string(g)
                .unwrap_or_default()
                .contains("planeai-stop-notify")
        }) {
            return;
        }
        let mut group = serde_json::json!({ "hooks": [hook_entry] });
        if let Some(m) = matcher {
            group
                .as_object_mut()
                .unwrap()
                .insert("matcher".to_string(), serde_json::json!(m));
        }
        arr.push(group);
    };

    ensure_hook("Stop", None);
    ensure_hook("StopFailure", None);
    ensure_hook("Notification", Some("idle_prompt|permission_prompt"));
    ensure_hook("UserPromptSubmit", None);
    // Flips the session back to busy after a permission prompt is approved.
    ensure_hook("PostToolUse", None);

    let output = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_path, output)
        .map_err(|e| format!("failed to write settings.json: {e}"))?;
    Ok(())
}

pub fn install_codex_hook(home: &str) -> Result<(), String> {
    let codex_dir = codex_dir(Path::new(home));
    let hooks_dir = codex_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_name, script_content) = (
        "planeai-stop-notify-codex.sh",
        include_str!("../resources/planeai-stop-notify-codex.sh"),
    );
    #[cfg(windows)]
    let (script_name, script_content) = (
        "planeai-stop-notify-codex.ps1",
        include_str!("../resources/planeai-stop-notify-codex.ps1"),
    );
    let script_path = hooks_dir.join(script_name);
    std::fs::write(&script_path, script_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    let custom_home = std::env::var_os("CODEX_HOME").is_some();
    install_codex_hook_at(&codex_dir, &codex_script_command(&script_path, custom_home))
}

/// Shell command Codex runs for the notify script, without the event argument.
fn codex_script_command(script_path: &Path, custom_codex_home: bool) -> String {
    if cfg!(windows) {
        format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
            script_path.display()
        )
    } else if custom_codex_home {
        // POSIX single quotes so `$`, backticks or `"` in CODEX_HOME stay literal.
        format!(
            "'{}'",
            script_path.display().to_string().replace('\'', r"'\''")
        )
    } else {
        // Keep `$HOME` unexpanded (Codex runs hooks through a shell) so a hooks.json
        // kept in dotfiles stays portable across machines.
        let script_name = script_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        format!("\"$HOME/.codex/hooks/{script_name}\"")
    }
}

/// Merge planeai's entries into `<codex_dir>/hooks.json`, preserving other hooks.
///
/// Groups are only ever appended: Codex keys hook trust by `event:group:index`,
/// so reordering existing groups would invalidate the user's trust decisions.
/// The file is only rewritten when entries are missing, and in place (no temp
/// file + rename) so a symlinked hooks.json keeps pointing at its target.
pub fn install_codex_hook_at(codex_dir: &Path, script_command: &str) -> Result<(), String> {
    std::fs::create_dir_all(codex_dir).map_err(|e| format!("failed to create codex dir: {e}"))?;
    let hooks_path = codex_dir.join("hooks.json");

    let mut config: serde_json::Value = match std::fs::read_to_string(&hooks_path) {
        Ok(content) if !content.trim().is_empty() => serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse hooks.json: {e}"))?,
        _ => serde_json::json!({}),
    };
    let original = config.clone();
    let hooks = config
        .as_object_mut()
        .ok_or("hooks.json is not a JSON object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("hooks.json `hooks` is not a JSON object")?;

    for (event, arg) in CODEX_HOOK_EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.json `hooks.{event}` is not an array"))?;
        if !groups.iter().any(codex_group_has_marker) {
            groups.push(serde_json::json!({
                "hooks": [{ "type": "command", "command": format!("{script_command} {arg}") }]
            }));
        }
    }

    if config == original && hooks_path.exists() {
        return Ok(());
    }
    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&hooks_path, output + "\n")
        .map_err(|e| format!("failed to write hooks.json: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kind_from_command_matches_known_binaries() {
        for (command, expected) in [
            ("kiro-cli chat --trust-all-tools", Some(AgentKind::Kiro)),
            ("claude", Some(AgentKind::Claude)),
            ("/opt/homebrew/bin/claude --resume", Some(AgentKind::Claude)),
            ("copilot --resume", Some(AgentKind::Copilot)),
            ("npx copilot", Some(AgentKind::Copilot)),
            ("env FOO=1 claude", Some(AgentKind::Claude)),
            (r"C:\Tools\claude.exe", Some(AgentKind::Claude)),
            (
                "\"/Applications/My Tools/kiro-cli\" chat",
                Some(AgentKind::Kiro),
            ),
        ] {
            assert_eq!(AgentKind::from_command(command), expected, "{command}");
        }
    }

    #[test]
    fn agent_kind_from_command_rejects_lookalikes() {
        for command in ["aider", "my-claude-wrapper", "claudette", "", "   "] {
            assert_eq!(AgentKind::from_command(command), None, "{command}");
        }
    }

    const SUPERSET_HOOKS: &str = r#"{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "superset notify.sh" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "superset notify.sh" }] }]
  }
}"#;

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn codex_commands_opt_out_of_the_shared_daemon() {
        for (command, expected) in [
            ("codex", "codex --no-daemon"),
            ("codex --yolo", "codex --no-daemon --yolo"),
            ("codex resume --last", "codex --no-daemon resume --last"),
            (
                "/opt/homebrew/bin/codex --model o3",
                "/opt/homebrew/bin/codex --no-daemon --model o3",
            ),
            ("npx codex", "npx codex --no-daemon"),
        ] {
            assert_eq!(codex_without_daemon(command, |_| true), expected);
        }
    }

    #[test]
    fn codex_daemon_probe_runs_the_launcher_words() {
        let mut probed = None;
        codex_without_daemon("npx codex --yolo", |launcher| {
            probed = Some(launcher.to_string());
            true
        });
        assert_eq!(probed.as_deref(), Some("npx codex"));
    }

    #[test]
    fn codex_daemon_flag_is_skipped_when_unsupported_or_already_chosen() {
        assert_eq!(
            codex_without_daemon("codex --yolo", |_| false),
            "codex --yolo"
        );
        for command in [
            "codex --no-daemon",
            "codex --remote ws://host:1234",
            "codex --remote=ws://host:1234",
            "claude --resume",
            "my-codex-wrapper",
        ] {
            let result = codex_without_daemon(command, |_| panic!("probed {command}"));
            assert_eq!(result, command);
        }
    }

    #[cfg(unix)]
    #[test]
    fn help_mentions_reads_the_launcher_help_output() {
        assert!(help_mentions("echo usage: --no-daemon", "--no-daemon"));
        assert!(!help_mentions("echo usage:", "--no-daemon"));
        assert!(!help_mentions("planeai-missing-binary", "--no-daemon"));
    }

    #[test]
    fn agent_kind_from_command_detects_codex() {
        assert_eq!(AgentKind::from_command("codex"), Some(AgentKind::Codex));
        assert_eq!(
            AgentKind::from_command("codex resume --last"),
            Some(AgentKind::Codex)
        );
        assert_eq!(AgentKind::from_command("my-codex-wrapper"), None);
    }

    #[test]
    fn install_codex_hook_registers_all_events() {
        let dir = tempfile::tempdir().unwrap();
        install_codex_hook_at(dir.path(), "\"/x/planeai-stop-notify-codex.sh\"").unwrap();
        let v = read_json(&dir.path().join("hooks.json"));
        for (event, arg) in [
            ("UserPromptSubmit", "busy"),
            ("PostToolUse", "busy"),
            ("Stop", "stop"),
        ] {
            assert_eq!(
                v["hooks"][event][0]["hooks"][0],
                serde_json::json!({
                    "type": "command",
                    "command": format!("\"/x/planeai-stop-notify-codex.sh\" {arg}")
                }),
                "{event}"
            );
        }
        assert!(v["hooks"].get("PermissionRequest").is_none());
        assert!(is_codex_hook_installed_at(dir.path()));
    }

    #[test]
    fn install_codex_hook_appends_after_existing_groups() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hooks.json"), SUPERSET_HOOKS).unwrap();
        install_codex_hook_at(dir.path(), "planeai-stop-notify-codex.sh").unwrap();
        let v = read_json(&dir.path().join("hooks.json"));
        assert_eq!(
            v["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "superset notify.sh"
        );
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "superset notify.sh");
        assert_eq!(
            stop[1]["hooks"][0]["command"],
            "planeai-stop-notify-codex.sh stop"
        );
    }

    #[test]
    fn install_codex_hook_is_idempotent_and_skips_unchanged_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        install_codex_hook_at(dir.path(), "planeai-stop-notify-codex.sh").unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, first.replace("  ", "\t")).unwrap();
        install_codex_hook_at(dir.path(), "planeai-stop-notify-codex.sh").unwrap();
        let second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            second,
            first.replace("  ", "\t"),
            "no rewrite when complete"
        );
        assert_eq!(
            read_json(&path)["hooks"]["Stop"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn install_codex_hook_rejects_invalid_json_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(install_codex_hook_at(dir.path(), "planeai-stop-notify-codex.sh").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[cfg(unix)]
    #[test]
    fn install_codex_hook_preserves_symlinked_hooks_json() {
        let dir = tempfile::tempdir().unwrap();
        let dotfiles = dir.path().join("dotfiles/hooks.json");
        std::fs::create_dir_all(dotfiles.parent().unwrap()).unwrap();
        std::fs::write(&dotfiles, SUPERSET_HOOKS).unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::os::unix::fs::symlink(&dotfiles, codex_dir.join("hooks.json")).unwrap();

        install_codex_hook_at(&codex_dir, "planeai-stop-notify-codex.sh").unwrap();

        let link = std::fs::symlink_metadata(codex_dir.join("hooks.json")).unwrap();
        assert!(link.file_type().is_symlink());
        assert!(is_codex_hook_installed_at(&codex_dir));
        assert_eq!(
            read_json(&dotfiles)["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn detect_codex_hook_requires_every_event() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_codex_hook_installed_at(dir.path()));
        std::fs::write(
            dir.path().join("hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"planeai-stop-notify-codex.sh stop"}]}]}}"#,
        )
        .unwrap();
        assert!(!is_codex_hook_installed_at(dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn codex_script_command_keeps_home_unexpanded_unless_codex_home_is_custom() {
        let script = Path::new("/Users/me/.codex/hooks/planeai-stop-notify-codex.sh");
        assert_eq!(
            codex_script_command(script, false),
            "\"$HOME/.codex/hooks/planeai-stop-notify-codex.sh\""
        );
        let custom = Path::new("/opt/it's $HOME/hooks/planeai-stop-notify-codex.sh");
        assert_eq!(
            codex_script_command(custom, true),
            r"'/opt/it'\''s $HOME/hooks/planeai-stop-notify-codex.sh'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_codex_hook_writes_executable_script() {
        if std::env::var_os("CODEX_HOME").is_some() {
            return; // would write into the developer's real Codex home
        }
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        install_codex_hook(home).unwrap();

        let codex_dir = dir.path().join(".codex");
        use std::os::unix::fs::PermissionsExt;
        let script = codex_dir.join("hooks/planeai-stop-notify-codex.sh");
        let mode = std::fs::metadata(&script).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(AgentKind::Codex.is_hook_installed(home));
    }

    #[cfg(unix)]
    #[test]
    fn codex_script_sends_event_and_prints_nothing() {
        use std::io::BufRead;
        if std::process::Command::new("nc").arg("-h").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("planeai-stop-notify-codex.sh");
        std::fs::write(
            &script,
            include_str!("../resources/planeai-stop-notify-codex.sh"),
        )
        .unwrap();
        let sock = dir.path().join("n.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();

        for (arg, expected) in [("busy", "busy"), ("stop", "stop")] {
            let output = std::process::Command::new("bash")
                .arg(&script)
                .arg(arg)
                .env("PLANEAI_SESSION_ID", "s-1")
                .env("PLANEAI_SOCKET", &sock)
                .output()
                .unwrap();
            assert!(output.status.success());
            assert!(output.stdout.is_empty(), "stdout must stay empty for {arg}");

            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream)
                .read_line(&mut line)
                .unwrap();
            assert_eq!(
                line.trim(),
                format!(r#"{{"session_id":"s-1","event":"{expected}"}}"#)
            );
        }
    }

    /// Runs a bundled notify script inside a fake planeai session and returns
    /// the line it sent to the notify socket, if any.
    #[cfg(unix)]
    fn run_notify_script(script: &str, arg: Option<&str>, stdin: &str) -> Option<String> {
        use std::io::{BufRead, Write};
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("hook.sh");
        std::fs::write(&script_path, script).unwrap();
        let sock = dir.path().join("n.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        listener.set_nonblocking(true).unwrap();

        let mut child = std::process::Command::new("bash")
            .arg(&script_path)
            .args(arg)
            .env("PLANEAI_SESSION_ID", "s-1")
            .env("PLANEAI_SOCKET", &sock)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty(), "hook scripts must print nothing");

        let (stream, _) = listener.accept().ok()?;
        stream.set_nonblocking(false).unwrap();
        let mut line = String::new();
        std::io::BufReader::new(stream)
            .read_line(&mut line)
            .unwrap();
        Some(line.trim().to_string())
    }

    #[cfg(unix)]
    fn has_command(name: &str) -> bool {
        std::process::Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[cfg(unix)]
    #[test]
    fn notify_scripts_ignore_unknown_events() {
        if !has_command("nc") {
            return;
        }
        let codex = include_str!("../resources/planeai-stop-notify-codex.sh");
        let copilot = include_str!("../resources/planeai-stop-notify-copilot.sh");
        assert_eq!(run_notify_script(codex, Some("bogus"), ""), None);
        assert_eq!(run_notify_script(codex, Some("notification"), ""), None);
        assert_eq!(run_notify_script(copilot, Some("bogus"), ""), None);

        if !has_command("jq") {
            return;
        }
        let claude = include_str!("../resources/planeai-stop-notify-claude.sh");
        let kiro = include_str!("../resources/planeai-stop-notify.sh");
        for script in [claude, kiro] {
            let unknown = r#"{"hook_event_name":"SessionStart"}"#;
            assert_eq!(run_notify_script(script, None, unknown), None);
            assert_eq!(run_notify_script(script, None, "not json"), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn claude_script_maps_events() {
        if !has_command("nc") || !has_command("jq") {
            return;
        }
        let claude = include_str!("../resources/planeai-stop-notify-claude.sh");
        for (event, expected) in [
            ("UserPromptSubmit", "busy"),
            ("PostToolUse", "busy"),
            ("Stop", "stop"),
            ("Notification", "notification"),
            ("StopFailure", "notification"),
        ] {
            let stdin = format!(r#"{{"hook_event_name":"{event}"}}"#);
            assert_eq!(
                run_notify_script(claude, None, &stdin),
                Some(format!(r#"{{"session_id":"s-1","event":"{expected}"}}"#)),
                "{event}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn codex_script_is_a_noop_outside_planeai_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("planeai-stop-notify-codex.sh");
        std::fs::write(
            &script,
            include_str!("../resources/planeai-stop-notify-codex.sh"),
        )
        .unwrap();
        let output = std::process::Command::new("bash")
            .arg(&script)
            .arg("stop")
            .env_remove("PLANEAI_SESSION_ID")
            .env("PATH", "/usr/bin:/bin")
            .env_remove("TMUX")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn refresh_skips_agents_without_installed_hooks() {
        let dir = tempfile::tempdir().unwrap();
        refresh_hook_scripts(dir.path().to_str().unwrap());
        assert!(!dir.path().join(".kiro").exists());
        assert!(!dir.path().join(".claude").exists());
        assert!(!dir.path().join(".codex").exists());
    }

    // ─── Hook detection tests ────────────────────────────────────────────────

    #[test]
    fn detect_kiro_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        let config = serde_json::json!({
            "name": "default",
            "hooks": {
                "stop": [{ "command": "/path/to/planeai-stop-notify.sh" }],
                "userPromptSubmit": [{ "command": "/path/to/planeai-stop-notify.sh" }]
            }
        });
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        assert!(is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        std::fs::write(&config_path, r#"{"name":"default"}"#).unwrap();
        assert!(!is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_kiro_hook_installed_at(&dir.path().join("nope.json")));
    }

    #[test]
    fn detect_kiro_hook_pascal_case_keys() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("default.json");
        let config = serde_json::json!({
            "name": "default",
            "hooks": {
                "Stop": [{ "command": "/path/to/planeai-stop-notify.sh" }],
                "UserPromptSubmit": [{ "command": "/path/to/planeai-stop-notify.sh" }]
            }
        });
        std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
        assert!(is_kiro_hook_installed_at(&config_path));
    }

    #[test]
    fn detect_kiro_v3_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let hook = serde_json::json!({
            "version": "v1",
            "hooks": [
                { "name": "planeai-stop", "trigger": "Stop", "action": { "type": "command", "command": "/path/to/planeai-stop-notify.sh" } },
                { "name": "planeai-busy", "trigger": "UserPromptSubmit", "action": { "type": "command", "command": "/path/to/planeai-stop-notify.sh" } }
            ]
        });
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            serde_json::to_string(&hook).unwrap(),
        )
        .unwrap();
        assert!(is_kiro_v3_hook_installed_at(&hooks_dir));
    }

    #[test]
    fn detect_kiro_v3_hook_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_kiro_v3_hook_installed_at(&dir.path().join("hooks")));
    }

    #[test]
    fn detect_kiro_v3_hook_missing_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        let hook = serde_json::json!({
            "version": "v1",
            "hooks": [
                { "name": "planeai-stop", "trigger": "Stop", "action": { "type": "command", "command": "/path/to/planeai-stop-notify.sh" } }
            ]
        });
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            serde_json::to_string(&hook).unwrap(),
        )
        .unwrap();
        assert!(!is_kiro_v3_hook_installed_at(&hooks_dir));
    }

    #[test]
    fn detect_claude_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/path/to/planeai-stop-notify-claude.sh"}]}],"StopFailure":[{}],"Notification":[{}],"UserPromptSubmit":[{}]}}"#).unwrap();
        assert!(is_claude_hook_installed_at(&path));
    }

    #[test]
    fn detect_claude_hook_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"hooks":{}}"#).unwrap();
        assert!(!is_claude_hook_installed_at(&path));
    }

    #[test]
    fn detect_copilot_hook_installed() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(
            hooks_dir.join("planeai-notify.json"),
            r#"{"hooks":{"agentStop":[{"bash":"planeai-stop-notify-copilot.sh stop"}]}}"#,
        )
        .unwrap();
        assert!(is_copilot_hook_installed_at(dir.path()));
    }

    #[test]
    fn detect_copilot_hook_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_copilot_hook_installed_at(dir.path()));
    }

    // ─── Hook install tests ──────────────────────────────────────────────────

    #[test]
    fn install_copilot_hook_creates_correct_structure() {
        let dir = tempfile::tempdir().unwrap();
        install_copilot_hook_at(dir.path(), "/path/script.sh", "/path/script.ps1").unwrap();
        let content =
            std::fs::read_to_string(dir.path().join("hooks/planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["version"], 1);
        assert!(v["hooks"]["agentStop"][0]["bash"]
            .as_str()
            .unwrap()
            .contains("stop"));
    }

    #[test]
    fn install_copilot_hook_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        install_copilot_hook_at(dir.path(), "/path/s.sh", "/path/s.ps1").unwrap();
        install_copilot_hook_at(dir.path(), "/path/s.sh", "/path/s.ps1").unwrap();
        let content =
            std::fs::read_to_string(dir.path().join("hooks/planeai-notify.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["hooks"]["agentStop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_claude_hook_creates_correct_structure() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        install_claude_hook_at(&claude_dir, "/path/planeai-stop-notify-claude.sh").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
    }

    #[test]
    fn install_claude_hook_marks_post_tool_use_busy() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        install_claude_hook_at(&claude_dir, "/path/planeai-stop-notify-claude.sh").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
        assert!(include_str!("../resources/planeai-stop-notify-claude.sh").contains("PostToolUse"));
    }

    #[test]
    fn install_claude_hook_merges_with_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();
        install_claude_hook_at(&claude_dir, "/path/planeai-stop-notify-claude.sh").unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["permissions"]["allow"][0], "Read");
        assert!(settings["hooks"]["Stop"].is_array());
    }

    #[test]
    fn install_kiro_hook_creates_both_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();

        install_kiro_hook(home).unwrap();

        let config_path = dir.path().join(".kiro/agents/default.json");
        let content = std::fs::read_to_string(&config_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Both hooks registered
        let stop = &v["hooks"]["stop"];
        assert!(stop.is_array());
        assert!(stop[0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));

        let prompt = &v["hooks"]["userPromptSubmit"];
        assert!(prompt.is_array());
        assert!(prompt[0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
    }

    #[test]
    fn install_kiro_hook_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();

        install_kiro_hook(home).unwrap();
        install_kiro_hook(home).unwrap();

        let config_path = dir.path().join(".kiro/agents/default.json");
        let content = std::fs::read_to_string(&config_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should not duplicate entries
        assert_eq!(v["hooks"]["stop"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["userPromptSubmit"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn install_kiro_hook_preserves_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();
        let agents_dir = dir.path().join(".kiro/agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        // Pre-existing config with user hooks
        let existing = serde_json::json!({
            "name": "default",
            "tools": ["*"],
            "hooks": {
                "preToolUse": [{ "matcher": "shell", "command": "guardrails check" }]
            }
        });
        std::fs::write(
            agents_dir.join("default.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install_kiro_hook(home).unwrap();

        let content = std::fs::read_to_string(agents_dir.join("default.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Existing hooks preserved
        assert_eq!(v["hooks"]["preToolUse"][0]["matcher"], "shell");
        // Our hooks added
        assert!(v["hooks"]["stop"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
        assert!(v["hooks"]["userPromptSubmit"][0]["command"]
            .as_str()
            .unwrap()
            .contains("planeai-stop-notify"));
    }

    #[test]
    fn install_kiro_hook_writes_script_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap();

        install_kiro_hook(home).unwrap();

        #[cfg(not(windows))]
        let script_path = dir.path().join(".kiro/hooks/planeai-stop-notify.sh");
        #[cfg(windows)]
        let script_path = dir.path().join(".kiro/hooks/planeai-stop-notify.ps1");
        assert!(script_path.exists());

        let content = std::fs::read_to_string(&script_path).unwrap();
        assert!(content.contains("PLANEAI_SESSION_ID"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&script_path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o755);
        }
    }
}
