use tauri::ipc::Channel;
use tauri::State;

use crate::commands::sessions::lifecycle::session_lifecycle_event;
use crate::config;
use crate::db;
use crate::plugins::PluginRuntimeHandle;
use crate::pty;
use crate::state::{ConfigState, DbState, NotifyHandle, ProjectOperationState, PtyState};

use super::helpers::{build_local_env, provider_has_hook};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn attach_session(
    session_id: String,
    dark_mode: Option<bool>,
    on_data: Channel<tauri::ipc::Response>,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
    state: State<'_, PtyState>,
    notify: State<'_, NotifyHandle>,
    runtime: State<'_, PluginRuntimeHandle>,
    operations: State<'_, ProjectOperationState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // Every database and config read below runs through `commands::blocking`: on
    // macOS WKWebView dispatches IPC handlers on the main thread, so doing this
    // work inline stalls PTY delivery and keystrokes for every other session.
    let project_id = crate::commands::blocking({
        let db = db_state.0.clone();
        let session_id = session_id.clone();
        move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            Ok(db::get_session(&conn, &session_id)
                .map_err(|e| e.to_string())?
                .ok_or("session not found")?
                .project_id)
        }
    })
    .await?;
    let operation_lock = operations.lock_for(&project_id);
    let _operation_guard = operation_lock.lock_owned().await;

    // One trip off the main thread resolves everything the attach needs. The
    // session is re-read here, still under the per-project operation guard: a
    // deletion that wins the race removes the session before an attach can create
    // a new local process.
    // Config lives in memory, so snapshotting it is a brief lock rather than I/O —
    // the thing AGENTS.md allows a lock to be held for. Cloning it lets the whole
    // database phase move off the main thread in one hop.
    let cfg = {
        let guard = config_state.0.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    let AttachPlan {
        session,
        pty_target,
        env,
        notification,
    } = crate::commands::blocking({
        let db = db_state.0.clone();
        let session_id = session_id.clone();
        move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            resolve_attach_plan(&conn, &cfg, &session_id, dark_mode)
        }
    })
    .await?;

    tracing::info!(
        session_id = %session_id,
        backend = %session.backend,
        status = %session.status,
        task_key = ?session.task_key,
        "attach_session"
    );

    state
        .0
        .attach(&session_id, pty_target, app.clone(), on_data, env)?;

    {
        let (display_name, project_name, hook_enabled) = &notification;
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&session_id, display_name, project_name, *hook_enabled);
    }

    let was_exited = session.status == "exited";
    crate::commands::blocking({
        let db = db_state.0.clone();
        let session_id = session_id.clone();
        move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            db::mark_attached(&conn, &session_id).map_err(|e| e.to_string())?;
            if was_exited {
                db::restore_session(&conn, &session_id).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    })
    .await?;

    if was_exited {
        runtime
            .0
            .dispatch_session_lifecycle(session_lifecycle_event(&session, "exited", "active"));
    }

    Ok(())
}

/// What an attach needs, resolved from the database and config in one pass.
///
/// Extracted from `attach_session` so the backend-specific resolution can be
/// tested: the command itself needs a Tauri runtime and an IPC channel, which is
/// why nothing in `commands/` tests the entry points directly.
#[derive(Debug)]
pub struct AttachPlan {
    pub session: db::Session,
    pub pty_target: pty::PtyTarget,
    pub env: Vec<(String, String)>,
    /// Display name, project name, and whether the provider has a notify hook.
    pub notification: (String, String, bool),
}

/// Resolve the PTY target, environment, and notification data for a session.
///
/// Pure with respect to the world apart from the connection it is handed: it reads
/// and never writes, so it is safe to call before the attach is committed.
pub fn resolve_attach_plan(
    conn: &rusqlite::Connection,
    cfg: &config::Config,
    session_id: &str,
    dark_mode: Option<bool>,
) -> Result<AttachPlan, String> {
    let session = db::get_session(conn, session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    // Resolve pty_target and agent_command based on backend type
    let (pty_target, resolved_agent_command) = if session.backend == "tmux" {
        let tmux_name = session
            .tmux_name
            .clone()
            .ok_or("tmux session has no tmux_name")?;
        (pty::PtyTarget::TmuxAttach { tmux_name }, None)
    } else if session.backend == "daemon" {
        let socket_path = planeai_ipc::daemon_socket_path();
        (
            pty::PtyTarget::Daemon {
                session_id: session_id.to_string(),
                socket_path,
            },
            None,
        )
    } else if session.backend == planeai_rmux::BACKEND {
        // Resolve the recorded pane here so the PTY layer stays free of DB
        // access. A missing record means the agent is gone — a restarted
        // daemon invalidates every pane id — so the session is treated as
        // exited rather than silently attaching to nothing.
        let record = crate::rmux_resources::get(conn, session_id)
            .map_err(|e| e.to_string())?
            .ok_or("rmux session has no recorded pane; restart it")?;
        let workspace = record
            .workspace()
            .ok_or("rmux session has an unrecognised workspace")?;
        (
            pty::PtyTarget::Rmux {
                pty_key: session_id.to_string(),
                workspace,
                handle: record.handle(),
            },
            None,
        )
    } else {
        let provider_key = session.provider.as_deref().unwrap_or(&cfg.default_provider);
        let provider_def = cfg
            .providers
            .get(provider_key)
            .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;

        let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
        let project_path = projects
            .iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.path.as_str())
            .unwrap_or("/");
        let cwd = session
            .worktree_path
            .as_deref()
            .unwrap_or(project_path)
            .to_string();

        let is_first_attach = !session.attached_once;

        let cmd = if is_first_attach {
            config::launch_command(provider_def, session.auto_approve)
        } else {
            config::restart_command_for_provider(provider_def)
        };

        let target = pty::PtyTarget::Shell {
            command: cmd.clone(),
            cwd,
        };
        (target, Some(cmd))
    };

    // Build env via prepare_session() for local/tmux targets (canonical PATH
    // augmentation). Daemon and rmux targets don't need env here — the agent
    // process is spawned with its own environment at launch time, not at
    // attach time.
    let env = if session.backend != "daemon" && session.backend != planeai_rmux::BACKEND {
        let extra_path_dirs = cfg.resolved_extra_path_dirs();
        let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
        let project_path = projects
            .iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.path.as_str())
            .unwrap_or("/");

        let cwd = if session.backend == "tmux" {
            std::env::temp_dir()
        } else {
            std::path::PathBuf::from(session.worktree_path.as_deref().unwrap_or(project_path))
        };

        let agent_command = resolved_agent_command
            .as_deref()
            .unwrap_or("tmux attach-session");

        build_local_env(
            session_id,
            cwd,
            agent_command,
            dark_mode.unwrap_or(true),
            extra_path_dirs,
        )?
    } else {
        vec![]
    };

    // Gathered here rather than after the attach so the notify registration
    // needs no second database round trip.
    let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
    let project_name = projects
        .iter()
        .find(|p| p.id == session.project_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let display_name = if session.name.is_empty() {
        session.branch.clone()
    } else {
        session.name.clone()
    };
    let hook_enabled = session
        .provider
        .as_deref()
        .map(|pk| provider_has_hook(pk, cfg))
        .unwrap_or(false);

    Ok(AttachPlan {
        session,
        pty_target,
        env,
        notification: (display_name, project_name, hook_enabled),
    })
}

#[tauri::command]
pub async fn write_to_pty(
    session_id: String,
    data: Vec<u8>,
    state: State<'_, PtyState>,
) -> Result<bool, String> {
    match state.0.write(&session_id, &data).await {
        Ok(()) => Ok(true),
        Err(e) if e == "session not attached" => Ok(false),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn resize_pty(
    session_id: String,
    rows: u16,
    cols: u16,
    state: State<PtyState>,
) -> Result<(), String> {
    match state.0.resize(&session_id, rows, cols) {
        Err(e) if e == "session not attached" => Ok(()),
        other => other,
    }
}

#[tauri::command]
pub fn pause_pty(session_id: String, state: State<PtyState>) -> Result<(), String> {
    match state.0.pause(&session_id) {
        Err(e) if e == "session not attached" => Ok(()),
        other => other,
    }
}

#[tauri::command]
pub fn resume_pty(session_id: String, state: State<PtyState>) -> Result<(), String> {
    match state.0.resume(&session_id) {
        Err(e) if e == "session not attached" => Ok(()),
        other => other,
    }
}

#[tauri::command]
pub fn check_session_alive(tmux_name: String) -> bool {
    #[cfg(not(windows))]
    {
        crate::tmux::has_session(&tmux_name)
    }
    #[cfg(windows)]
    {
        let _ = tmux_name;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database with one project and one session on `backend`.
    fn db_with_session(backend: &str, tmux_name: Option<&str>) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        crate::rmux_resources::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "demo", "/repos/demo").unwrap();
        db::create_session_with_id(
            &conn,
            "session-1",
            &project.id,
            "PLA-1: demo",
            tmux_name,
            "feature",
            Some("/repos/demo-wt"),
            None,
            backend,
            false,
            Some("PLA-1"),
            None,
            None,
        )
        .unwrap();
        conn
    }

    fn config_with_provider() -> config::Config {
        // `Config::default()` already carries the built-in providers, which is what
        // a local-backend attach resolves its command from.
        config::Config::default()
    }

    #[test]
    fn a_tmux_session_attaches_to_its_tmux_name() {
        let conn = db_with_session("tmux", Some("planeai-session-1"));

        let plan = resolve_attach_plan(&conn, &config_with_provider(), "session-1", Some(true))
            .expect("tmux plan");

        match plan.pty_target {
            pty::PtyTarget::TmuxAttach { tmux_name } => {
                assert_eq!(tmux_name, "planeai-session-1");
            }
            other => panic!("expected a tmux attach, got {other:?}"),
        }
        // tmux attaches a local process, so it needs the augmented environment.
        assert!(
            plan.env.iter().any(|(key, _)| key == "PATH"),
            "tmux attach should build a local env"
        );
    }

    #[test]
    fn a_tmux_session_without_a_tmux_name_is_refused() {
        // Attaching with nothing to attach to would spawn a stray process.
        let conn = db_with_session("tmux", None);

        let error = resolve_attach_plan(&conn, &config_with_provider(), "session-1", None)
            .expect_err("a tmux session with no name cannot attach");

        assert!(error.contains("tmux_name"), "{error}");
    }

    #[test]
    fn a_daemon_session_attaches_over_the_daemon_socket_without_env() {
        let conn = db_with_session("daemon", None);

        let plan =
            resolve_attach_plan(&conn, &config_with_provider(), "session-1", None).expect("plan");

        match plan.pty_target {
            pty::PtyTarget::Daemon { session_id, .. } => assert_eq!(session_id, "session-1"),
            other => panic!("expected a daemon target, got {other:?}"),
        }
        // The agent already has its own environment from launch time; building one
        // here would imply this attach spawns the process, which it does not.
        assert!(plan.env.is_empty(), "daemon attach should not build env");
    }

    #[test]
    fn an_rmux_session_attaches_to_its_recorded_pane_without_env() {
        let conn = db_with_session(planeai_rmux::BACKEND, None);
        let workspace =
            planeai_rmux::WorkspaceKey::for_session("proj", Some("PLA-1"), "session-1").name();
        crate::rmux_resources::put(
            &conn,
            "session-1",
            "session-1",
            &workspace,
            planeai_rmux::ResourceHandle::from_u32(7),
        )
        .unwrap();

        let plan =
            resolve_attach_plan(&conn, &config_with_provider(), "session-1", None).expect("plan");

        match plan.pty_target {
            pty::PtyTarget::Rmux {
                pty_key,
                workspace: resolved,
                handle,
            } => {
                assert_eq!(pty_key, "session-1");
                assert_eq!(resolved, workspace);
                assert_eq!(handle.as_u32(), 7);
            }
            other => panic!("expected an rmux target, got {other:?}"),
        }
        assert!(plan.env.is_empty(), "rmux attach should not build env");
    }

    #[test]
    fn an_rmux_session_with_no_recorded_pane_is_refused() {
        // A restarted daemon invalidates every pane id, so a missing record means
        // the agent is gone. Attaching anyway would render an empty terminal that
        // looks alive.
        let conn = db_with_session(planeai_rmux::BACKEND, None);

        let error = resolve_attach_plan(&conn, &config_with_provider(), "session-1", None)
            .expect_err("no recorded pane means no attach");

        assert!(error.contains("no recorded pane"), "{error}");
    }

    #[test]
    fn an_rmux_session_whose_workspace_is_unrecognised_is_refused() {
        let conn = db_with_session(planeai_rmux::BACKEND, None);
        // A workspace name PlaneAI no longer owns must not be addressed.
        conn.execute(
            "INSERT INTO rmux_resources (pty_key, session_id, workspace_key, pane_id, updated_at)
             VALUES ('session-1', 'session-1', 'someone-elses-session', 7, '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        let error = resolve_attach_plan(&conn, &config_with_provider(), "session-1", None)
            .expect_err("an unrecognised workspace cannot be attached");

        assert!(error.contains("unrecognised workspace"), "{error}");
    }

    #[test]
    fn a_local_session_launches_on_first_attach_and_restarts_afterwards() {
        // A local attach spawns a process, so its worktree has to exist: the env
        // builder validates the cwd rather than handing a bad one to the PTY.
        let worktree = tempfile::tempdir().unwrap();
        let worktree_path = worktree.path().display().to_string();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "demo", "/repos/demo").unwrap();
        db::create_session_with_id(
            &conn,
            "session-1",
            &project.id,
            "PLA-1: demo",
            None,
            "feature",
            Some(&worktree_path),
            None,
            "local",
            false,
            Some("PLA-1"),
            None,
            None,
        )
        .unwrap();

        let first = resolve_attach_plan(&conn, &config_with_provider(), "session-1", None)
            .expect("first attach");
        let launch_command = match &first.pty_target {
            pty::PtyTarget::Shell { command, cwd } => {
                // The worktree, not the project root: that is where the agent works.
                assert_eq!(cwd, &worktree_path);
                command.clone()
            }
            other => panic!("expected a shell target, got {other:?}"),
        };

        db::mark_attached(&conn, "session-1").unwrap();
        let second = resolve_attach_plan(&conn, &config_with_provider(), "session-1", None)
            .expect("second attach");
        let restart_command = match &second.pty_target {
            pty::PtyTarget::Shell { command, .. } => command.clone(),
            other => panic!("expected a shell target, got {other:?}"),
        };

        // A second attach must resume rather than start a fresh agent.
        assert_ne!(
            launch_command, restart_command,
            "attaching twice should not re-run the launch command"
        );
        assert!(first.env.iter().any(|(key, _)| key == "PATH"));
    }

    #[test]
    fn a_missing_session_is_refused_rather_than_attached() {
        let conn = db_with_session("tmux", Some("planeai-session-1"));

        let error = resolve_attach_plan(&conn, &config_with_provider(), "does-not-exist", None)
            .expect_err("a deleted session cannot be attached");

        assert!(error.contains("session not found"), "{error}");
    }

    #[test]
    fn the_notification_carries_the_session_and_project_names() {
        let conn = db_with_session("tmux", Some("planeai-session-1"));

        let plan =
            resolve_attach_plan(&conn, &config_with_provider(), "session-1", None).expect("plan");

        let (display_name, project_name, _hook_enabled) = plan.notification;
        assert_eq!(display_name, "PLA-1: demo");
        assert_eq!(project_name, "demo");
    }

    #[test]
    fn a_session_without_a_name_falls_back_to_its_branch() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "demo", "/repos/demo").unwrap();
        db::create_session_with_id(
            &conn,
            "session-1",
            &project.id,
            "",
            Some("planeai-session-1"),
            "feature/login",
            None,
            None,
            "tmux",
            false,
            None,
            None,
            None,
        )
        .unwrap();

        let plan =
            resolve_attach_plan(&conn, &config_with_provider(), "session-1", None).expect("plan");

        assert_eq!(plan.notification.0, "feature/login");
    }
}
