use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::cleanup;
use crate::config::Config;
use crate::db;
use crate::plugins::{PluginRuntimeHandle, PluginRuntimeSupervisor};
use crate::state::{ConfigState, DbState, McpServerState, PtyState};

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

/// A committed session status transition: the event plugins receive, and the provider
/// session to stop when the transition ends a plugin provider's session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionTransition {
    pub event: SessionLifecycleEvent,
    pub provider_session: Option<crate::plugin_providers::StopTarget>,
}

pub(crate) fn session_transition(
    session: &db::Session,
    previous_status: &str,
    status: &str,
) -> SessionTransition {
    SessionTransition {
        event: SessionLifecycleEvent {
            session_id: session.id.clone(),
            project_id: session.project_id.clone(),
            branch: session.branch.clone(),
            linked_task_key: session.task_key.clone(),
            previous_status: previous_status.to_string(),
            status: status.to_string(),
        },
        provider_session: session
            .provider
            .as_deref()
            .and_then(|key| crate::plugin_providers::StopTarget::of(&session.id, key)),
    }
}

/// Marks an active session exited, returning the transition to dispatch; `None` when it was not active.
pub(crate) fn exit_active_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<SessionTransition>, String> {
    let Some(session) = db::get_session(conn, session_id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if session.status != "active" {
        return Ok(None);
    }
    db::mark_session_exited(conn, session_id).map_err(|e| e.to_string())?;
    Ok(Some(session_transition(&session, "active", "exited")))
}

/// Restart off the main thread: probing and spawning talk to the rmux daemon.
/// Returns the session's status before the restart and the restarted session.
pub(crate) async fn restart_blocking(
    db: Arc<Mutex<Connection>>,
    cfg: Config,
    mcp: Option<planeai_core::mcp::McpLaunch>,
    session_id: String,
) -> Result<(String, db::Session), String> {
    crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let previous = db::get_session(&conn, &session_id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        let ops = crate::session_restart::real_restart_ops();
        let session =
            crate::session_restart::restart(&conn, &session_id, &cfg, mcp.as_ref(), &ops)?;
        Ok((previous.status, session))
    })
    .await
}

#[tauri::command]
pub async fn restart_session(
    session_id: String,
    app: AppHandle,
    db_state: State<'_, DbState>,
    config_state: State<'_, ConfigState>,
    runtime: State<'_, PluginRuntimeHandle>,
    mcp: State<'_, McpServerState>,
) -> Result<db::Session, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?.clone();
    let (previous_status, session) =
        restart_blocking(db_state.0.clone(), cfg, mcp.0.launch(), session_id.clone()).await?;
    runtime
        .0
        .dispatch_session_lifecycle(session_transition(&session, &previous_status, "active"));
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
            runtime.dispatch_session_lifecycle(session_transition(
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

    // Destroying ends backend processes, rmux's through a runtime of its own, which would
    // panic on an async worker: kept off them, as archiving is.
    let result = crate::commands::blocking(move || {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or("session not found")?;
        let result =
            crate::session_ops::destroy(&conn, &id, &Some(cfg), &cleanup::real_kill_ops())?;
        if session.status != "destroyed" {
            runtime.dispatch_session_lifecycle(session_transition(
                &session,
                &session.status,
                "destroyed",
            ));
        }
        Ok(result)
    })
    .await?;

    if !result.kill_errors.is_empty() {
        let msg = result.kill_errors.join("; ");
        let app = app_handle.clone();
        std::thread::spawn(move || {
            let _ = app.emit("app-error", msg);
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
            serde_json::to_value(session_transition(&session, "active", "archived").event).unwrap();
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
        let event = exit_active_session(&conn, "s1").unwrap().unwrap().event;
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
