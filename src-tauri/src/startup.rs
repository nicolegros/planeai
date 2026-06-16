use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

use crate::commands::pr::poll_pr_for_session;
use crate::commands::sessions::helpers::provider_has_hook;
use crate::config;
use crate::db;
use crate::notify::SharedNotifyState;
use crate::state::{ConfigState, DbState};

/// Revive sessions on startup: recreate dead tmux sessions.
/// Daemon sessions are managed by the daemon process and don't need reviving here.
pub fn revive_sessions<F, G>(
    conn: &rusqlite::Connection,
    cfg: &config::Config,
    has_tmux_session: F,
    create_tmux: G,
) -> Vec<String>
where
    F: Fn(&str) -> bool,
    G: Fn(&str, &str, &str, &str) -> Result<(), String>,
{
    let sessions = match db::list_sessions(conn) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let projects = db::list_projects(conn).unwrap_or_default();
    let mut failures = Vec::new();

    for session in &sessions {
        if session.backend != "tmux" {
            continue;
        }
        let tmux_name = match session.tmux_name.as_deref() {
            Some(n) => n,
            None => continue,
        };
        if has_tmux_session(tmux_name) {
            continue;
        }
        let provider_key = session.provider.as_deref().unwrap_or(&cfg.default_provider);
        let cmd = match cfg.providers.get(provider_key) {
            Some(provider_def) => config::restart_command_for_provider(
                provider_def,
                session.provider_session_id.as_deref(),
            ),
            None => continue,
        };
        let project_path = projects
            .iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.path.as_str())
            .unwrap_or("/");
        let cwd = session.worktree_path.as_deref().unwrap_or(project_path);

        match create_tmux(tmux_name, cwd, &cmd, &session.id) {
            Ok(()) => {
                if session.status == "exited" {
                    let _ = db::restore_session(conn, &session.id);
                }
            }
            Err(e) => {
                eprintln!(
                    "[revive] failed to recreate tmux for session {}: {}",
                    session.id, e
                );
                let _ = db::mark_session_exited(conn, &session.id);
                failures.push(session.id.clone());
            }
        }
    }

    failures
}

/// Reconcile daemon sessions: mark sessions as exited if the daemon doesn't know about them.
pub fn reconcile_daemon_sessions(conn: &rusqlite::Connection, _cfg: &config::Config) {
    let sessions = match db::list_sessions(conn) {
        Ok(s) => s,
        Err(_) => return,
    };

    let daemon_sessions: Vec<&db::Session> = sessions
        .iter()
        .filter(|s| s.backend == "daemon" && s.status == "active")
        .collect();

    if daemon_sessions.is_empty() {
        return;
    }

    let socket_path = planeai_ipc::daemon_socket_path();

    // Try to reach daemon and list sessions
    let daemon_list = tauri::async_runtime::block_on(async {
        // Don't spawn daemon just for reconciliation — if it's not running, sessions are dead
        if !socket_path.exists() {
            return None;
        }
        let mut client = crate::daemon_client::DaemonClient::connect(&socket_path)
            .await
            .ok()?;
        client.list_sessions().await.ok()
    });

    let alive_ids: std::collections::HashSet<String> = daemon_list
        .map(|list| list.into_iter().map(|s| s.session_id).collect())
        .unwrap_or_default();

    for session in daemon_sessions {
        if !alive_ids.contains(&session.id) {
            let _ = db::mark_session_exited(conn, &session.id);
        }
    }
}

/// Start a background task that listens for daemon exit events and marks sessions as exited.
pub fn start_daemon_event_listener(app_handle: &tauri::AppHandle) {
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let socket_path = planeai_ipc::daemon_socket_path();

        // Try connecting — if daemon isn't running, just return (will start when first session launches)
        let mut client = match crate::daemon_client::DaemonClient::connect(&socket_path).await {
            Ok(c) => c,
            Err(_) => return,
        };

        loop {
            match client.recv_event().await {
                Some(event) if event.event == "exited" => {
                    let db = app.state::<DbState>();
                    if let Ok(conn) = db.0.lock() {
                        let _ = db::mark_session_exited(&conn, &event.session_id);
                    }
                    let _ = app.emit(
                        "pty-exited",
                        serde_json::json!({ "pty_key": event.session_id }),
                    );
                    let _ = app.emit("sessions-changed", ());
                }
                Some(_) => {}  // Ignore unknown events
                None => break, // Connection closed
            }
        }
    });
}

/// Register all active sessions in NotifyState at startup (after NotifyState is created).
pub fn register_active_sessions(
    conn: &rusqlite::Connection,
    cfg: &config::Config,
    notify_state: &SharedNotifyState,
) {
    let sessions = match db::list_sessions(conn) {
        Ok(s) => s,
        Err(_) => return,
    };
    let projects = db::list_projects(conn).unwrap_or_default();
    let mut ns = notify_state.lock().unwrap();

    for session in &sessions {
        if session.status != "active" {
            continue;
        }
        let project_name = projects
            .iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.name.as_str())
            .unwrap_or("unknown");
        let display_name = if session.name.is_empty() {
            &session.branch
        } else {
            &session.name
        };
        let hook_enabled = session
            .provider
            .as_deref()
            .map(|pk| provider_has_hook(pk, cfg))
            .unwrap_or(false);
        ns.register_session(&session.id, display_name, project_name, hook_enabled);
    }
}

/// Start background PR status poller (every 2 minutes).
pub fn start_pr_poller(app_handle: &tauri::AppHandle) {
    let app_handle = app_handle.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(120));
        let db = app_handle.state::<DbState>();
        let cfg_state = app_handle.state::<ConfigState>();
        let Ok(conn) = db.0.lock() else { continue };
        let Ok(cfg) = cfg_state.0.lock() else {
            continue;
        };
        if cfg.pr_status.is_none() {
            continue;
        }
        let Ok(sessions) = db::list_sessions(&conn) else {
            continue;
        };
        let mut changed = false;
        for session in &sessions {
            match poll_pr_for_session(&conn, &cfg, session) {
                Ok(true) => changed = true,
                Err(e) => {
                    let _ = app_handle.emit("cleanup-error", e);
                }
                _ => {}
            }
        }
        if changed {
            let _ = app_handle.emit("sessions-changed", ());
        }
    });
}

/// Warm font cache in background so preferences page opens instantly.
pub fn warm_font_cache() {
    std::thread::spawn(|| {
        tauri::async_runtime::block_on(crate::commands::list_monospace_fonts()).ok();
    });
}

/// Initialize the symphony orchestrator if any project has auto_mode enabled.
pub fn init_symphony(
    app: &tauri::App,
    app_dir: &std::path::Path,
    db_arc: &Arc<Mutex<rusqlite::Connection>>,
) -> crate::symphony::SymphonyState {
    let conn = db_arc.lock().unwrap();
    let cfg_state = app.state::<ConfigState>();
    let cfg = cfg_state.0.lock().unwrap();

    let mut state = crate::symphony::SymphonyState::new();
    if let Some(orch_config) = crate::symphony::build_orchestrator_config(&cfg, &conn) {
        drop(cfg);
        drop(conn);

        let backend = Arc::new(crate::symphony::TauriBackend {
            db: db_arc.clone(),
            app_handle: app.handle().clone(),
            notify_socket: std::path::PathBuf::from(planeai::ipc::address(
                planeai::ipc::Channel::Notify,
                app_dir,
            )),
        });

        let token = tokio_util::sync::CancellationToken::new();
        let orchestrator = planeai_core::orchestrator::Orchestrator::new(orch_config, backend);
        let task_token = token.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let bridge_tx = tx.clone();

        let handle = tauri::async_runtime::spawn(async move {
            let _ = orchestrator.run(task_token, rx).await;
        });

        state.running = Some(crate::symphony::RunningOrchestrator {
            token: token.clone(),
            handle,
            command_tx: tx,
        });

        let app_handle = app.handle().clone();
        let watch_token = token;
        tauri::async_runtime::spawn(async move {
            watch_token.cancelled().await;
            let _ = app_handle.emit("symphony-stopped", ());
        });

        crate::symphony::start_ipc_bridge(app_dir, bridge_tx);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revive_recreates_dead_tmux_session_with_resume_command() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let _session = db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "test",
            Some("planeai-myapp-abc"),
            "main",
            Some("/tmp/worktree"),
            Some("kiro"),
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();
        db::set_provider_session_id(&conn, "s1", "sess-123").unwrap();

        let cfg = config::Config::default();
        let created = std::cell::RefCell::new(Vec::new());

        let failures = revive_sessions(
            &conn,
            &cfg,
            |_| false,
            |tmux_name, cwd, cmd, session_id| {
                created.borrow_mut().push((
                    tmux_name.to_string(),
                    cwd.to_string(),
                    cmd.to_string(),
                    session_id.to_string(),
                ));
                Ok(())
            },
        );

        assert!(failures.is_empty());
        let calls = created.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "planeai-myapp-abc");
        assert_eq!(calls[0].1, "/tmp/worktree");
        assert!(calls[0].2.contains("--resume-id"));
        assert!(calls[0].2.contains("sess-123"));
        assert_eq!(calls[0].3, "s1");
        assert_eq!(
            db::get_session(&conn, "s1").unwrap().unwrap().status,
            "active"
        );
    }

    #[test]
    fn revive_restores_exited_tmux_session_and_recreates_tmux() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let _session = db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "test",
            Some("planeai-myapp-abc"),
            "main",
            Some("/tmp/worktree"),
            Some("kiro"),
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, "s1").unwrap();

        let cfg = config::Config::default();
        let created = std::cell::RefCell::new(Vec::new());

        let failures = revive_sessions(
            &conn,
            &cfg,
            |_| false,
            |tmux_name, cwd, cmd, session_id| {
                created.borrow_mut().push((
                    tmux_name.to_string(),
                    cwd.to_string(),
                    cmd.to_string(),
                    session_id.to_string(),
                ));
                Ok(())
            },
        );

        assert!(failures.is_empty());
        assert_eq!(created.borrow().len(), 1);
        assert_eq!(
            db::get_session(&conn, "s1").unwrap().unwrap().status,
            "active"
        );
    }

    #[test]
    fn revive_skips_daemon_sessions() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let _session = db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "test",
            None,
            "main",
            Some("/tmp/worktree"),
            Some("kiro"),
            "daemon",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, "s1").unwrap();

        let cfg = config::Config::default();
        let created = std::cell::RefCell::new(Vec::new());

        let failures = revive_sessions(
            &conn,
            &cfg,
            |_| false,
            |tmux_name, cwd, cmd, session_id| {
                created.borrow_mut().push((
                    tmux_name.to_string(),
                    cwd.to_string(),
                    cmd.to_string(),
                    session_id.to_string(),
                ));
                Ok(())
            },
        );

        assert!(failures.is_empty());
        // No tmux session should be created for daemon sessions
        assert_eq!(created.borrow().len(), 0);
        // Daemon session stays exited (not restored)
        assert_eq!(
            db::get_session(&conn, "s1").unwrap().unwrap().status,
            "exited"
        );
    }

    #[test]
    fn revive_tmux_failure_marks_session_exited() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let _session = db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "test",
            Some("planeai-myapp-abc"),
            "main",
            Some("/tmp/worktree"),
            Some("kiro"),
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();

        let cfg = config::Config::default();

        let failures = revive_sessions(
            &conn,
            &cfg,
            |_| false,
            |_, _, _, _| Err("tmux not found".to_string()),
        );

        assert_eq!(failures, vec!["s1"]);
        assert_eq!(
            db::get_session(&conn, "s1").unwrap().unwrap().status,
            "exited"
        );
    }

    #[test]
    fn revive_leaves_alive_tmux_sessions_untouched() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        let _session = db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "test",
            Some("planeai-myapp-abc"),
            "main",
            Some("/tmp/worktree"),
            Some("kiro"),
            "tmux",
            false,
            None,
            None,
        )
        .unwrap();

        let cfg = config::Config::default();
        let created = std::cell::RefCell::new(Vec::new());

        let failures = revive_sessions(
            &conn,
            &cfg,
            |_| true,
            |tmux_name, cwd, cmd, session_id| {
                created.borrow_mut().push((
                    tmux_name.to_string(),
                    cwd.to_string(),
                    cmd.to_string(),
                    session_id.to_string(),
                ));
                Ok(())
            },
        );

        assert!(failures.is_empty());
        assert_eq!(created.borrow().len(), 0);
        assert_eq!(
            db::get_session(&conn, "s1").unwrap().unwrap().status,
            "active"
        );
    }

    #[test]
    fn register_active_sessions_registers_only_active() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        // Active session
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "active-sess",
            None,
            "main",
            None,
            None,
            "daemon",
            false,
            None,
            None,
        )
        .unwrap();
        // Archived session
        db::create_session_with_id(
            &conn,
            "s2",
            &project.id,
            "archived-sess",
            None,
            "feat",
            None,
            None,
            "daemon",
            false,
            None,
            None,
        )
        .unwrap();
        db::mark_session_exited(&conn, "s2").unwrap();

        let cfg = config::Config::default();
        let notify_state: SharedNotifyState =
            Arc::new(Mutex::new(crate::notify::NotifyState::new()));

        register_active_sessions(&conn, &cfg, &notify_state);

        let ns = notify_state.lock().unwrap();
        assert!(ns.get_meta("s1").is_some(), "active session registered");
        assert!(ns.get_meta("s2").is_none(), "exited session not registered");
        assert_eq!(ns.get_meta("s1").unwrap().name, "active-sess");
        assert_eq!(ns.get_meta("s1").unwrap().project_name, "myapp");
    }

    #[test]
    fn register_active_sessions_uses_branch_as_display_name_when_name_empty() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        planeai_tasks::sqlite::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", "/tmp/myapp").unwrap();
        db::create_session_with_id(
            &conn,
            "s1",
            &project.id,
            "", // empty name
            None,
            "feature/cool",
            None,
            None,
            "daemon",
            false,
            None,
            None,
        )
        .unwrap();

        let cfg = config::Config::default();
        let notify_state: SharedNotifyState =
            Arc::new(Mutex::new(crate::notify::NotifyState::new()));

        register_active_sessions(&conn, &cfg, &notify_state);

        let ns = notify_state.lock().unwrap();
        assert_eq!(ns.get_meta("s1").unwrap().name, "feature/cool");
    }
}
