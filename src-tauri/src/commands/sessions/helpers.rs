use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::config;
use crate::db;

use planeai_core::agent_hooks::AgentKind;
use planeai_core::session_launch::{prepare_session, CreateSessionRequest, SessionTarget};

static HOOK_CACHE: Mutex<Option<HashMap<AgentKind, bool>>> = Mutex::new(None);

/// Invalidate cached hook-installed results (call after hook installation).
pub fn invalidate_hook_cache() {
    *HOOK_CACHE.lock().unwrap() = None;
}

/// Resolve the working directory for a session's project.
pub(crate) fn session_cwd(conn: &rusqlite::Connection, session: &db::Session) -> Option<String> {
    if let Some(ref wt) = session.worktree_path {
        return Some(wt.clone());
    }
    db::get_project(conn, &session.project_id)
        .ok()
        .flatten()
        .map(|p| p.path)
}

/// Fire a task manager lifecycle hook (on_start, on_notify, on_restart, on_complete).
/// Delegates to the unified implementation in session_ops.
pub(crate) fn fire_task_hook(
    cfg: &config::Config,
    session: &db::Session,
    hook_name: &str,
    conn: &rusqlite::Connection,
) {
    crate::session_ops::fire_task_hook(cfg, session, hook_name, conn);
}

/// Check if a provider has hook-based idle detection.
pub(crate) fn provider_has_hook(provider_key: &str, cfg: &config::Config) -> bool {
    cfg.providers
        .get(provider_key)
        .and_then(|p| AgentKind::from_command(&p.command))
        .is_some_and(is_hook_installed)
}

pub(crate) fn is_hook_installed(kind: AgentKind) -> bool {
    let mut cache = HOOK_CACHE.lock().unwrap();
    *cache
        .get_or_insert_with(HashMap::new)
        .entry(kind)
        .or_insert_with(|| kind.is_hook_installed(&config::home_dir()))
}

/// Build the canonical PTY environment for a local/tmux session via `prepare_session()`.
///
/// Includes augmented PATH, TERM, PLANEAI_SESSION_ID, COLORFGBG, and PLANEAI_SOCKET.
pub(crate) fn build_local_env(
    session_id: &str,
    cwd: PathBuf,
    agent_command: &str,
    dark_mode: bool,
    extra_path_dirs: Vec<String>,
) -> Result<Vec<(String, String)>, String> {
    let mut pre_env = std::collections::HashMap::new();
    pre_env.insert(
        "COLORFGBG".to_string(),
        if dark_mode { "15;0" } else { "0;15" }.to_string(),
    );
    pre_env.insert(
        "PLANEAI_SOCKET".to_string(),
        planeai_ipc::address(planeai_ipc::Channel::Notify, &planeai_paths::app_data_dir()),
    );
    let req = CreateSessionRequest {
        session_id: session_id.to_string(),
        project_cwd: cwd,
        session_target: SessionTarget::Local,
        agent_command: agent_command.to_string(),
        env: pre_env,
        extra_path_dirs,
        cols: 80,
        rows: 24,
        durable_logs: std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok(),
    };
    let result = prepare_session(&req).map_err(|e| e.to_string())?;
    Ok(result.env.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_cache_returns_consistent_value() {
        let first = is_hook_installed(AgentKind::Kiro);
        let second = is_hook_installed(AgentKind::Kiro);
        assert_eq!(first, second);
    }

    #[test]
    fn invalidate_resets_all_caches() {
        for kind in AgentKind::ALL {
            is_hook_installed(kind);
        }
        invalidate_hook_cache();
        assert!(HOOK_CACHE.lock().unwrap().is_none());
    }

    #[test]
    fn provider_has_hook_unknown_provider_returns_false() {
        let cfg = config::Config {
            providers: std::collections::HashMap::new(),
            default_provider: "kiro".into(),
            ..Default::default()
        };
        assert!(!provider_has_hook("nonexistent", &cfg));
    }

    fn test_session(task_key: Option<&str>) -> db::Session {
        db::Session {
            id: "test-id".into(),
            project_id: "proj-1".into(),
            name: "test".into(),
            tmux_name: None,
            branch: "main".into(),
            status: "active".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree_path: None,
            provider: None,
            backend: "tmux".into(),
            provider_session_id: None,
            tab_count: 1,
            auto_approve: false,
            task_key: task_key.map(|s| s.to_string()),
            base_branch: None,
            pr_url: None,
            pr_state: None,
            attached_once: false,
            parent_session_id: None,
            task_project_id: None,
        }
    }

    #[test]
    fn fire_task_hook_no_task_key_returns_early() {
        let cfg = config::Config::default();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let session = test_session(None);
        // Should not panic — just returns early
        fire_task_hook(&cfg, &session, "on_complete", &conn);
    }

    #[test]
    fn fire_task_hook_no_task_management_returns_early() {
        let cfg = config::Config {
            task_management: None,
            ..config::Config::default()
        };
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let session = test_session(Some("PROJ-1"));
        // Should not panic — returns early when no task_management configured
        fire_task_hook(&cfg, &session, "on_complete", &conn);
    }

    #[test]
    fn fire_task_hook_with_matching_project_derives_prefix() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let cfg = config::Config {
            task_management: Some(config::TaskManager {
                templates: None,
                on_start: None,
                on_notify: None,
                on_restart: None,
                on_complete: Some(config::LifecycleHook {
                    move_to: "done".into(),
                }),
                auto_dispatch: None,
            }),
            ..config::Config::default()
        };

        let session = test_session(Some("MYA-1"));
        // Runs through the full path — project matched, prefix derived.
        // The task update won't find the task (no task tables), but doesn't error.
        fire_task_hook(&cfg, &session, "on_complete", &conn);
    }

    #[test]
    fn fire_task_hook_unknown_hook_name_does_nothing() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();

        let cfg = config::Config {
            task_management: Some(config::TaskManager {
                templates: None,
                on_start: None,
                on_notify: None,
                on_restart: None,
                on_complete: Some(config::LifecycleHook {
                    move_to: "done".into(),
                }),
                auto_dispatch: None,
            }),
            ..config::Config::default()
        };

        let session = test_session(Some("MYA-1"));
        // Unknown hook name — does nothing
        fire_task_hook(&cfg, &session, "on_unknown", &conn);
    }
}
