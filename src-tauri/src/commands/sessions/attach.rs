use tauri::ipc::Channel;
use tauri::State;

use crate::config;
use crate::db;
use crate::pty;
use crate::state::{ConfigState, DbState, NotifyHandle, PtyState};

use super::helpers::{build_local_env, provider_has_hook};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn attach_session(
    session_id: String,
    dark_mode: Option<bool>,
    on_data: Channel<tauri::ipc::Response>,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
    state: State<PtyState>,
    notify: State<NotifyHandle>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    // Resolve pty_target and agent_command based on backend type
    let (pty_target, resolved_agent_command) = if session.backend == "tmux" {
        let tmux_name = session.tmux_name.ok_or("tmux session has no tmux_name")?;
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
    } else {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
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

        let cmd = if session.status == "exited" {
            config::restart_command_for_provider(provider_def, None)
        } else {
            config::launch_command(provider_def, session.auto_approve)
        };

        let target = pty::PtyTarget::Shell {
            command: cmd.clone(),
            cwd,
        };
        (target, Some(cmd))
    };

    // Build env via prepare_session() for local/tmux targets (canonical PATH augmentation).
    // Daemon targets don't need env — the daemon process has its own.
    let env = if session.backend != "daemon" {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
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
            std::path::PathBuf::from(session.worktree_path.as_deref().unwrap_or(project_path))
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

    state
        .0
        .attach(&session_id, pty_target, app.clone(), on_data, env)?;

    {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
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
            .map(|pk| provider_has_hook(pk, &cfg))
            .unwrap_or(false);
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&session_id, display_name, project_name, hook_enabled);
    }

    if session.status == "exited" {
        db::restore_session(&conn, &session_id).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub fn write_to_pty(
    session_id: String,
    data: Vec<u8>,
    state: State<PtyState>,
) -> Result<(), String> {
    state.0.write(&session_id, &data)
}

#[tauri::command]
pub fn resize_pty(
    session_id: String,
    rows: u16,
    cols: u16,
    state: State<PtyState>,
) -> Result<(), String> {
    state.0.resize(&session_id, rows, cols)
}

#[tauri::command]
pub fn pause_pty(session_id: String, state: State<PtyState>) -> Result<(), String> {
    state.0.pause(&session_id)
}

#[tauri::command]
pub fn resume_pty(session_id: String, state: State<PtyState>) -> Result<(), String> {
    state.0.resume(&session_id)
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
