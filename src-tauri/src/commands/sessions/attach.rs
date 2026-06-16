use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};

use crate::config;
use crate::daemon_client;
use crate::db;
use crate::pty;
use crate::state::{ConfigState, DaemonSessions, DbState, NotifyHandle, PtyState};
use crate::util::resolve_command;

use super::helpers::provider_has_hook;
use super::launch::discover_provider_session_id;

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
    daemon_sessions: State<DaemonSessions>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    let is_daemon = session.backend == "daemon" || session.backend == "direct";

    let discovery_info = if session.backend != "tmux" {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let provider_key = session.provider.as_deref().unwrap_or(&cfg.default_provider);
        let provider_def = cfg
            .providers
            .get(provider_key)
            .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;

        let list_cmd = provider_def.list_sessions_command.clone();
        let pattern = provider_def.session_id_pattern.clone();

        let resume_id = session.provider_session_id.as_deref().and_then(|pid| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE provider_session_id = ?1 AND status = 'active' AND id != ?2",
                rusqlite::params![pid, &session_id],
                |r| r.get(0),
            ).unwrap_or(0);
            if count > 0 { None } else { Some(pid) }
        });
        let is_resume = resume_id.is_some() && provider_def.resume_flag.is_some();

        let cmd = if is_resume {
            config::restart_command_for_provider(provider_def, resume_id)
        } else {
            config::launch_command(provider_def, session.auto_approve)
        };
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let command = resolve_command(parts[0]);
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

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

        Some((
            list_cmd,
            pattern,
            is_resume,
            session.provider_session_id.clone(),
            cwd.clone(),
            command,
            args,
            cwd,
        ))
    } else {
        None
    };

    if is_daemon {
        // Daemon backend: ensure daemon is running, then connect to it
        daemon_client::ensure_daemon()?;
        let mut daemon_conn =
            daemon_client::DaemonConn::connect().map_err(|e| format!("daemon connect: {e}"))?;

        // Check if session already exists in daemon (re-attach case)
        let list_resp = daemon_conn.list_sessions().unwrap_or_default();
        let session_exists = list_resp.contains(&session_id);

        if !session_exists {
            // Need to create the session in the daemon
            let (_, _, _, _, _, ref command, ref args, ref cwd) = discovery_info.as_ref().unwrap();
            let env: Vec<(String, String)> = vec![
                ("TERM".to_string(), "xterm-256color".to_string()),
                (
                    "COLORFGBG".to_string(),
                    if dark_mode.unwrap_or(true) {
                        "15;0"
                    } else {
                        "0;15"
                    }
                    .to_string(),
                ),
            ];
            daemon_conn
                .create_session(&session_id, command, args, cwd, &env)
                .map_err(|e| format!("daemon create: {e}"))?;
        }

        // Attach to data stream from daemon
        let data_stream = daemon_conn
            .attach(&session_id)
            .map_err(|e| format!("daemon attach: {e}"))?;

        // Spawn a thread to read from daemon and forward to frontend
        let sid = session_id.clone();
        let app_clone = app.clone();
        std::thread::spawn(move || {
            let mut ds = data_stream;
            loop {
                match ds.read_frame() {
                    Some(data) => {
                        if on_data.send(tauri::ipc::Response::new(data)).is_err() {
                            break;
                        }
                    }
                    None => {
                        // Session exited or disconnected
                        let _ = app_clone.emit("pty-exited", serde_json::json!({ "pty_key": sid }));
                        break;
                    }
                }
            }
        });

        // Track this session as daemon-managed
        daemon_sessions.0.lock().unwrap().insert(session_id.clone());
    } else {
        // tmux backend: use local PTY as before
        let pty_target = {
            let tmux_name = session
                .tmux_name
                .clone()
                .ok_or("tmux session has no tmux_name")?;
            pty::PtyTarget::TmuxAttach { tmux_name }
        };

        state.0.attach(
            &session_id,
            pty_target,
            dark_mode.unwrap_or(true),
            app.clone(),
            on_data,
        )?;
    }

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
    drop(conn);

    if let Some((Some(list_cmd), Some(pattern), is_resume, previous_id, cwd, _, _, _)) =
        discovery_info
    {
        eprintln!(
            "[DEBUG-disc] spawning discovery thread for session={}, list_cmd='{}', cwd='{}'",
            &session_id, &list_cmd, &cwd
        );
        let sid = session_id.clone();
        let db_path = app
            .path()
            .app_data_dir()
            .expect("app data dir")
            .join("planeai.db");
        std::thread::spawn(move || {
            discover_provider_session_id(
                &sid,
                &list_cmd,
                &pattern,
                &cwd,
                previous_id.as_deref(),
                is_resume,
                &db_path,
                &app,
            );
        });
    } else {
        eprintln!(
            "[DEBUG-disc] skipping discovery: discovery_info={:?}",
            discovery_info
                .as_ref()
                .map(|(a, b, _, _, _, _, _, _)| (a.is_some(), b.is_some()))
        );
    }

    Ok(())
}

#[tauri::command]
pub fn write_to_pty(
    session_id: String,
    data: Vec<u8>,
    state: State<PtyState>,
    daemon_sessions: State<DaemonSessions>,
) -> Result<(), String> {
    if daemon_sessions.0.lock().unwrap().contains(&session_id) {
        let mut conn = crate::daemon_client::DaemonConn::connect()
            .map_err(|e| format!("daemon connect: {e}"))?;
        conn.write_to_session(&session_id, &data)
    } else {
        state.0.write(&session_id, &data)
    }
}

#[tauri::command]
pub fn resize_pty(
    session_id: String,
    rows: u16,
    cols: u16,
    state: State<PtyState>,
    daemon_sessions: State<DaemonSessions>,
) -> Result<(), String> {
    if daemon_sessions.0.lock().unwrap().contains(&session_id) {
        let mut conn = crate::daemon_client::DaemonConn::connect()
            .map_err(|e| format!("daemon connect: {e}"))?;
        conn.resize_session(&session_id, rows, cols)
    } else {
        state.0.resize(&session_id, rows, cols)
    }
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
