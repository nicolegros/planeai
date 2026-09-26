use std::collections::HashSet;

use tauri::State;

use planeai_core::agent_hooks::AgentKind;

use crate::commands::sessions::helpers::{invalidate_hook_cache, is_hook_installed};
use crate::config;
use crate::state::ConfigState;

fn configured_agent_kinds(cfg: &config::Config) -> HashSet<AgentKind> {
    cfg.providers
        .values()
        .filter_map(|p| AgentKind::from_command(&p.command))
        .collect()
}

#[tauri::command]
pub async fn is_notify_hook_installed(
    config_state: State<'_, ConfigState>,
) -> Result<bool, String> {
    let kinds = configured_agent_kinds(&config_state.0.lock().unwrap());
    super::blocking(move || Ok(kinds.into_iter().all(is_hook_installed))).await
}

#[tauri::command]
pub async fn install_notify_hook(config_state: State<'_, ConfigState>) -> Result<(), String> {
    let kinds = configured_agent_kinds(&config_state.0.lock().unwrap());
    super::blocking(move || {
        let home = config::home_dir();
        let result = kinds
            .into_iter()
            .try_for_each(|kind| kind.install_hook(&home));
        invalidate_hook_cache();
        result
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_agent_kinds_dedupes_and_skips_unknown_providers() {
        let mut cfg = config::Config::default();
        cfg.providers.insert(
            "claude-yolo".into(),
            config::Provider {
                command: "claude --model opus".into(),
                ..Default::default()
            },
        );
        cfg.providers.insert(
            "aider".into(),
            config::Provider {
                command: "aider".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            configured_agent_kinds(&cfg),
            HashSet::from([AgentKind::Kiro, AgentKind::Claude, AgentKind::Copilot])
        );
    }
}
