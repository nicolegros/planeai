//! Notification hooks installed into each supported agent CLI's own config.

use std::path::{Path, PathBuf};

/// A CLI coding agent planeai knows how to install notification hooks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    Kiro,
    Claude,
    Copilot,
}

impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::Kiro, AgentKind::Claude, AgentKind::Copilot];

    fn binary_name(self) -> &'static str {
        match self {
            AgentKind::Kiro => "kiro",
            AgentKind::Claude => "claude",
            AgentKind::Copilot => "copilot",
        }
    }

    /// Identify the agent a provider command launches, e.g. `kiro-cli chat`,
    /// `/opt/homebrew/bin/claude --resume` or `npx copilot`.
    ///
    /// Matches a command word whose file name is the agent name or starts with
    /// `<name>-`, so wrappers like `my-claude-wrapper` are not misdetected.
    pub fn from_command(command: &str) -> Option<Self> {
        command.split_whitespace().find_map(|word| {
            let file_name = word
                .trim_matches(|c| c == '"' || c == '\'')
                .rsplit(['/', '\\'])
                .next()?;
            let name = file_name.strip_suffix(".exe").unwrap_or(file_name);
            Self::ALL.into_iter().find(|kind| {
                let bin = kind.binary_name();
                name == bin
                    || name
                        .strip_prefix(bin)
                        .is_some_and(|rest| rest.starts_with('-'))
            })
        })
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
        }
    }

    /// Install (or repair) the hook script and register it in the agent's config.
    /// Idempotent: existing user config is preserved and planeai entries are never duplicated.
    pub fn install_hook(self, home: &str) -> Result<(), String> {
        match self {
            AgentKind::Kiro => install_kiro_hook(home),
            AgentKind::Claude => install_claude_hook(home),
            AgentKind::Copilot => install_copilot_hook(home),
        }
    }
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

    let output = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_path, output)
        .map_err(|e| format!("failed to write settings.json: {e}"))?;
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

    #[test]
    fn refresh_skips_agents_without_installed_hooks() {
        let dir = tempfile::tempdir().unwrap();
        refresh_hook_scripts(dir.path().to_str().unwrap());
        assert!(!dir.path().join(".kiro").exists());
        assert!(!dir.path().join(".claude").exists());
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
