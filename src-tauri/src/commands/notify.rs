use tauri::State;

use crate::config;
use crate::notify;
use crate::state::ConfigState;

use crate::commands::sessions::helpers::{
    invalidate_hook_cache, is_claude_hook_installed, is_copilot_hook_installed,
    is_kiro_hook_installed,
};

#[tauri::command]
pub fn is_notify_hook_installed(config_state: State<ConfigState>) -> bool {
    let cfg = config_state.0.lock().unwrap();
    let supported: Vec<_> = cfg
        .providers
        .values()
        .filter(|p| {
            p.command.contains("kiro")
                || p.command.contains("claude")
                || p.command.contains("copilot")
        })
        .collect();
    if supported.is_empty() {
        return true;
    }
    supported.iter().all(|p| {
        if p.command.contains("kiro") {
            is_kiro_hook_installed()
        } else if p.command.contains("claude") {
            is_claude_hook_installed()
        } else {
            is_copilot_hook_installed()
        }
    })
}

#[tauri::command]
pub fn install_notify_hook(config_state: State<ConfigState>) -> Result<(), String> {
    let cfg = config_state.0.lock().unwrap();
    let home = config::home_dir();

    if cfg.providers.values().any(|p| p.command.contains("kiro")) {
        install_kiro_hook(&home)?;
    }

    if cfg.providers.values().any(|p| p.command.contains("claude")) {
        install_claude_hook(&home)?;
    }

    if cfg
        .providers
        .values()
        .any(|p| p.command.contains("copilot"))
    {
        install_copilot_hook(&home)?;
    }

    invalidate_hook_cache();

    Ok(())
}

fn install_kiro_hook(home: &str) -> Result<(), String> {
    let hooks_dir = format!("{home}/.kiro/hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.sh"),
        include_str!("../../resources/planeai-stop-notify.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.ps1"),
        include_str!("../../resources/planeai-stop-notify.ps1"),
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

fn install_claude_hook(home: &str) -> Result<(), String> {
    let claude_dir = std::path::PathBuf::from(format!("{home}/.claude"));
    let hooks_dir = claude_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.sh"),
        include_str!("../../resources/planeai-stop-notify-claude.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.ps1"),
        include_str!("../../resources/planeai-stop-notify-claude.ps1"),
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
    notify::install_claude_hook_at(&claude_dir, &script_command)
}

fn install_copilot_hook(home: &str) -> Result<(), String> {
    let copilot_dir = std::env::var("COPILOT_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(format!("{home}/.copilot")));
    let hooks_dir = copilot_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (bash_path, bash_content) = (
        hooks_dir.join("planeai-stop-notify-copilot.sh"),
        include_str!("../../resources/planeai-stop-notify-copilot.sh"),
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
    let ps_content = include_str!("../../resources/planeai-stop-notify-copilot.ps1");
    std::fs::write(&ps_path, ps_content)
        .map_err(|e| format!("failed to write hook script: {e}"))?;

    notify::install_copilot_hook_at(
        &copilot_dir,
        &bash_path.to_string_lossy(),
        &ps_path.to_string_lossy(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
