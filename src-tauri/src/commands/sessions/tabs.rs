use tauri::ipc::Channel;
use tauri::State;

use crate::config;
use crate::db;
use crate::pty;
use crate::state::{ConfigState, DbState, PtyState};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn spawn_tab(
    session_id: String,
    tab_index: u32,
    dark_mode: Option<bool>,
    on_data: Channel<tauri::ipc::Response>,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
    state: State<PtyState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
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

    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "cmd.exe".to_string()
        } else {
            "/bin/zsh".to_string()
        }
    });

    let pty_key = format!("{}:{}", session_id, tab_index);

    let target = if session.backend == "daemon" {
        // Check if shell tab already exists in daemon (reattach after app restart)
        let already_running = crate::daemon_client::list_sessions_sync()
            .map(|ids| ids.contains(&pty_key))
            .unwrap_or(false);
        if !already_running {
            crate::daemon::spawn_session(&pty_key, &shell, &[], &cwd, None)?;
        }
        let socket_path = planeai_ipc::daemon_socket_path();
        pty::PtyTarget::Daemon {
            session_id: pty_key.clone(),
            socket_path,
        }
    } else {
        pty::PtyTarget::Shell {
            command: format!("{} -l", shell),
            cwd: cwd.clone(),
        }
    };

    // Build env for local shell tabs via prepare_session()
    let env = if session.backend != "daemon" {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let extra_path_dirs = cfg.resolved_extra_path_dirs();
        super::helpers::build_local_env(
            &pty_key,
            std::path::PathBuf::from(&cwd),
            &format!("{} -l", shell),
            dark_mode.unwrap_or(true),
            extra_path_dirs,
        )?
    } else {
        vec![]
    };

    state.0.attach(&pty_key, target, app, on_data, env)?;

    Ok(())
}

#[tauri::command]
pub fn close_tab(
    session_id: String,
    tab_index: u32,
    db_state: State<DbState>,
    state: State<PtyState>,
) -> Result<(), String> {
    let pty_key = format!("{}:{}", session_id, tab_index);
    state.0.detach(&pty_key);

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    // Kill daemon shell session if daemon backend
    if session.backend == "daemon" {
        let kill_ops = crate::cleanup::real_kill_ops();
        let _ = (kill_ops.kill_daemon_session)(&pty_key);
    }

    let new_count = (session.tab_count - 1).max(1);
    db::update_tab_count(&conn, &session_id, new_count).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn increment_tab_count(session_id: String, db_state: State<DbState>) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    db::update_tab_count(&conn, &session_id, session.tab_count + 1).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn check_tmux_available() -> bool {
    config::tmux_available()
}
