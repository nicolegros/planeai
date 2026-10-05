use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::cleanup;
use crate::config::Config;
use crate::db;
use crate::plugins::{PluginRuntimeHandle, PluginRuntimeSupervisor};
use crate::state::{ConfigState, DbState, PtyState};

/// A session status transition, as plugins receive it in `plugin.sessionLifecycle`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename = "status_changed")]
pub(crate) struct SessionLifecycleEvent {
    pub session_id: String,
    pub project_id: String,
    pub branch: String,
    pub linked_task_key: Option<String>,
    pub previous_status: String,
    pub status: String,
}

pub(crate) fn session_lifecycle_event(
    session: &db::Session,
    previous_status: &str,
    status: &str,
) -> SessionLifecycleEvent {
    SessionLifecycleEvent {
        session_id: session.id.clone(),
        project_id: session.project_id.clone(),
        branch: session.branch.clone(),
        linked_task_key: session.task_key.clone(),
        previous_status: previous_status.to_string(),
        status: status.to_string(),
    }
}

/// Marks an active session exited, returning the event to dispatch; `None` when it was not active.
pub(crate) fn exit_active_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<SessionLifecycleEvent>, String> {
    let Some(session) = db::get_session(conn, session_id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if session.status != "active" {
        return Ok(None);
    }
    db::mark_session_exited(conn, session_id).map_err(|e| e.to_string())?;
    Ok(Some(session_lifecycle_event(&session, "active", "exited")))
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
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    let (db, runtime) = (db_state.0.clone(), runtime.0.clone());

    // Destroying removes the worktree and ends backend processes, rmux's through a runtime of
    // its own, which would panic on an async worker: kept off them, as archiving is.
    let result = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        let result = crate::session_ops::destroy(&conn, &id, &Some(cfg), &cleanup::real_ops())?;
        if session.status != "destroyed" {
            runtime.dispatch_session_lifecycle(session_lifecycle_event(
                &session,
                &session.status,
                "destroyed",
            ));
        }
        Ok(result)
    })
    .await?;

    if !result.cleanup_errors.is_empty() {
        let msg = result.cleanup_errors.join("; ");
        let app = app_handle.clone();
        std::thread::spawn(move || {
            let _ = app.emit("cleanup-error", msg);
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(conn: &Connection) -> db::Session {
        db::migrate(conn).unwrap();
        planeai_tasks::sqlite::migrate(conn).unwrap();
        let project = db::create_project(conn, "app", "/tmp/app").unwrap();
        db::create_session_with_id(
            conn,
            "s1",
            &project.id,
            "chat",
            None,
            "feat/chat",
            None,
            Some("chat:claude"),
            "plugin",
            false,
            Some("PLA-1"),
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn lifecycle_events_keep_their_wire_shape() {
        let conn = Connection::open_in_memory().unwrap();
        let session = session(&conn);
        let event =
            serde_json::to_value(session_lifecycle_event(&session, "active", "archived")).unwrap();
        assert_eq!(
            event,
            serde_json::json!({
                "type": "status_changed",
                "session_id": "s1",
                "project_id": session.project_id,
                "branch": "feat/chat",
                "linked_task_key": "PLA-1",
                "previous_status": "active",
                "status": "archived",
            })
        );
    }

    #[test]
    fn only_an_active_session_exits() {
        let conn = Connection::open_in_memory().unwrap();
        session(&conn);
        let event = exit_active_session(&conn, "s1").unwrap().unwrap();
        assert_eq!(
            (event.previous_status.as_str(), event.status.as_str()),
            ("active", "exited")
        );
        assert_eq!(
            db::get_session(&conn, "s1").unwrap().unwrap().status,
            "exited"
        );
        // Already exited, or unknown: nothing changes and nothing is announced.
        assert_eq!(exit_active_session(&conn, "s1").unwrap(), None);
        assert_eq!(exit_active_session(&conn, "missing").unwrap(), None);
    }
}
