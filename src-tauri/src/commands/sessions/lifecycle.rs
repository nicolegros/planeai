use tauri::{Emitter, State};

use crate::cleanup;
use crate::db;
use crate::state::{ConfigState, DbState, PtyState};

#[tauri::command]
pub fn restart_session(
    session_id: String,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
) -> Result<db::Session, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let ops = crate::session_ops::real_restart_ops();
    crate::session_ops::restart(&conn, &session_id, &cfg, &ops)
}

#[tauri::command]
pub fn archive_session(
    id: String,
    db_state: State<DbState>,
    pty_state: State<PtyState>,
    config_state: State<ConfigState>,
) -> Result<(), String> {
    pty_state.0.detach(&id);
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    crate::session_ops::archive(&conn, &id, &Some(cfg), &cleanup::real_kill_ops())?;
    Ok(())
}

#[tauri::command]
pub async fn destroy_session(
    id: String,
    db_state: State<'_, DbState>,
    pty_state: State<'_, PtyState>,
    config_state: State<'_, ConfigState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    pty_state.0.detach(&id);

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();

    let result = crate::session_ops::destroy(&conn, &id, &Some(cfg), &cleanup::real_ops())?;

    if !result.cleanup_errors.is_empty() {
        let msg = result.cleanup_errors.join("; ");
        let app = app_handle.clone();
        std::thread::spawn(move || {
            let _ = app.emit("cleanup-error", msg);
        });
    }

    Ok(())
}
