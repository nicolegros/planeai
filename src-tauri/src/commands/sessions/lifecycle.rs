use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::cleanup;
use crate::config::Config;
use crate::db;
use crate::plugins::{PluginRuntimeHandle, PluginRuntimeSupervisor};
use crate::state::{ConfigState, DbState, PtyState};

pub(crate) fn session_lifecycle_event(
    session: &db::Session,
    previous_status: &str,
    status: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "status_changed",
        "session_id": session.id,
        "project_id": session.project_id,
        "branch": session.branch,
        "linked_task_key": session.task_key,
        "previous_status": previous_status,
        "status": status,
    })
}

#[tauri::command]
pub fn restart_session(
    session_id: String,
    app: AppHandle,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
    runtime: State<PluginRuntimeHandle>,
) -> Result<db::Session, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let previous = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let ops = crate::session_restart::real_restart_ops();
    let session = crate::session_restart::restart(&conn, &session_id, &cfg, &ops)?;
    runtime
        .0
        .dispatch_session_lifecycle(session_lifecycle_event(
            &session,
            &previous.status,
            "active",
        ));
    // Emit event so frontend re-attaches the PTY to the restarted session
    let _ = app.emit(
        "session-restarted",
        serde_json::json!({ "session_id": session_id }),
    );
    Ok(session)
}

#[tauri::command]
pub async fn archive_session(
    id: String,
    db_state: State<'_, DbState>,
    pty_state: State<'_, PtyState>,
    config_state: State<'_, ConfigState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<(), String> {
    pty_state.0.detach(&id);
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    archive_off_main_thread(id, db_state.0.clone(), runtime.0.clone(), Some(cfg)).await
}

#[tauri::command]
pub async fn park_session(
    id: String,
    db_state: State<'_, DbState>,
    pty_state: State<'_, PtyState>,
    runtime: State<'_, PluginRuntimeHandle>,
) -> Result<(), String> {
    pty_state.0.detach(&id);
    // Parking closes an agent without declaring its linked task complete.
    archive_off_main_thread(id, db_state.0.clone(), runtime.0.clone(), None).await
}

/// Archiving kills backend processes over daemon IPC, which must not stall the
/// main thread that delivers PTY output.
async fn archive_off_main_thread(
    id: String,
    db: Arc<Mutex<Connection>>,
    runtime: Arc<PluginRuntimeSupervisor>,
    cfg: Option<Config>,
) -> Result<(), String> {
    crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        crate::session_ops::archive(&conn, &id, &cfg, &cleanup::real_kill_ops())?;
        if session.status != "archived" {
            runtime.dispatch_session_lifecycle(session_lifecycle_event(
                &session,
                &session.status,
                "archived",
            ));
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn destroy_session(
    id: String,
    db_state: State<'_, DbState>,
    pty_state: State<'_, PtyState>,
    config_state: State<'_, ConfigState>,
    runtime: State<'_, PluginRuntimeHandle>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    pty_state.0.detach(&id);

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();

    let result = crate::session_ops::destroy(&conn, &id, &Some(cfg), &cleanup::real_ops())?;
    if session.status != "destroyed" {
        runtime
            .0
            .dispatch_session_lifecycle(session_lifecycle_event(
                &session,
                &session.status,
                "destroyed",
            ));
    }

    if !result.cleanup_errors.is_empty() {
        let msg = result.cleanup_errors.join("; ");
        let app = app_handle.clone();
        std::thread::spawn(move || {
            let _ = app.emit("cleanup-error", msg);
        });
    }

    Ok(())
}
