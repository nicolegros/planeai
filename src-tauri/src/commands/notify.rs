use tauri::State;

use crate::config;
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
        planeai_core::notify::install_kiro_hook(&home)?;
    }

    if cfg.providers.values().any(|p| p.command.contains("claude")) {
        planeai_core::notify::install_claude_hook(&home)?;
    }

    if cfg
        .providers
        .values()
        .any(|p| p.command.contains("copilot"))
    {
        planeai_core::notify::install_copilot_hook(&home)?;
    }

    invalidate_hook_cache();

    Ok(())
}

/// Returns the current agent states for all sessions (polled by frontend).
#[tauri::command]
pub fn get_agent_states(
    notify: State<crate::state::NotifyHandle>,
) -> std::collections::HashMap<String, String> {
    let s = notify.0.lock().unwrap();
    let mut result = std::collections::HashMap::new();
    for (id, state) in s.all_states() {
        result.insert(id.clone(), format!("{:?}", state));
    }
    result
}

#[cfg(test)]
mod tests {
    use planeai_core::notify::install_kiro_hook;

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
