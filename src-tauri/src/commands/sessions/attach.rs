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
    let (session, pty_target, env, notification) = crate::commands::blocking({
        let db = db_state.0.clone();
        let session_id = session_id.clone();
        move || {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let session = db::get_session(&conn, &session_id)
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
                        session_id: session_id.clone(),
                        socket_path,
                    },
                    None,
                )
            } else if session.backend == planeai_rmux::BACKEND {
                // Resolve the recorded pane here so the PTY layer stays free of DB
                // access. A missing record means the agent is gone — a restarted
                // daemon invalidates every pane id — so the session is treated as
                // exited rather than silently attaching to nothing.
                let record = crate::rmux_resources::get(&conn, &session_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("rmux session has no recorded pane; restart it")?;
                let workspace = record
                    .workspace()
                    .ok_or("rmux session has an unrecognised workspace")?;
                (
                    pty::PtyTarget::Rmux {
                        pty_key: session_id.clone(),
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

                let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
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
                let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
                let project_path = projects
                    .iter()
                    .find(|p| p.id == session.project_id)
                    .map(|p| p.path.as_str())
                    .unwrap_or("/");

                let cwd = if session.backend == "tmux" {
                    std::env::temp_dir()
                } else {
                    std::path::PathBuf::from(
                        session.worktree_path.as_deref().unwrap_or(project_path),
                    )
                };

                let agent_command = resolved_agent_command
                    .as_deref()
                    .unwrap_or("tmux attach-session");

                build_local_env(
                    &session_id,
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
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
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
                .map(|pk| provider_has_hook(pk, &cfg))
                .unwrap_or(false);

            Ok((
                session,
                pty_target,
                env,
                (display_name, project_name, hook_enabled),
            ))
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
