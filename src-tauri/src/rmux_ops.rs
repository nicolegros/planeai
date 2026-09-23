//! Synchronous rmux operations for callers outside the Tauri runtime.
//!
//! The CLI, auto-dispatch, cleanup, and restart all run on blocking threads with
//! no access to the app's async runtime, so each call gets a short-lived
//! current-thread runtime. Keeping that bridge in one place stops it from being
//! reinvented per call site.
//!
//! These helpers also own the pairing between an rmux pane and PlaneAI's
//! `pty_key`, so a caller never has to remember to record it.

use std::collections::HashMap;

use planeai_rmux::{ResourceHandle, ResourceSpawn, WorkspaceKey, WorkspaceName};
use rusqlite::Connection;

/// Open a connection for recording the pane mapping.
///
/// These are infrequent control operations on blocking threads, so opening a
/// connection per call is cheaper than threading one through every caller — and
/// several callers (cleanup, restart ops) have no connection to thread.
fn db() -> Result<Connection, String> {
    Connection::open(planeai_paths::db_path()).map_err(|error| error.to_string())
}

/// Cursor label for rmux reads. Distinct from `tmux:` and `daemon:` so a cursor
/// issued by one backend is rejected rather than reinterpreted.
// Reached from the library by `planeai-cli` and `task_cli`; the Tauri binary
// compiles its own copy of this module where those callers are absent.
#[allow(dead_code)]
pub const CURSOR_LABEL: &str = "rmux";

/// Default geometry for a resource created before any terminal has attached.
pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;

/// Bridge a synchronous caller to the shared async rmux client.
///
/// The operation may run twice: a cached connection can be dead by the time it is
/// used, and [`crate::rmux_client::with_retry`] reconnects and retries once. That
/// is why this takes `Fn` rather than `FnOnce` — callers clone what they capture.
pub fn blocking<Operation, Fut, T>(operation: Operation) -> Result<T, String>
where
    Operation: Fn(std::sync::Arc<planeai_rmux::RmuxClient>) -> Fut,
    Fut: std::future::Future<Output = planeai_rmux::Result<T>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(crate::rmux_client::with_retry(operation))
}

/// Spawn a resource and record which pane hosts it.
///
/// `env` is borrowed as `&str` pairs because callers build it with
/// `planeai_core::command::build_daemon_env`, which returns borrowed values.
pub fn spawn_resource_blocking(
    pty_key: &str,
    session_id: &str,
    workspace: &WorkspaceName,
    command: &str,
    cwd: &str,
    env: &HashMap<&str, &str>,
) -> Result<ResourceHandle, String> {
    // Reuse a resource that is still alive. Without this a second launch attempt
    // would add another window running the same agent, because the workspace
    // already exists and every spawn creates a window.
    if let Some(existing) = live_handle(pty_key) {
        tracing::info!(pty_key, "rmux resource already live; reusing");
        return Ok(existing);
    }

    let spawn = ResourceSpawn {
        workspace: workspace.clone(),
        pty_key: pty_key.to_string(),
        command: command.to_string(),
        cwd: cwd.to_string(),
        env: env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect(),
        cols: DEFAULT_COLS,
        rows: DEFAULT_ROWS,
    };

    let handle = blocking(move |client| {
        let spawn = spawn.clone();
        async move { client.spawn_resource(&spawn).await }
    })?;

    // Record after the spawn succeeds so a failed launch leaves no stale pane id.
    crate::rmux_resources::put(&db()?, pty_key, session_id, workspace, handle)
        .map_err(|error| error.to_string())?;
    Ok(handle)
}

/// The recorded handle for a resource, if its pane is still on the daemon.
fn live_handle(pty_key: &str) -> Option<ResourceHandle> {
    let conn = db().ok()?;
    let record = crate::rmux_resources::get(&conn, pty_key).ok()??;
    let live = blocking(|client| async move { client.live_pane_ids().await }).ok()?;
    live.contains(&record.pane_id).then(|| record.handle())
}

/// Send a prompt to a session's agent pane.
///
/// `send_text` is the literal `send-keys -l` path, so the text reaches the tty
/// unmodified; the trailing newline submits it, matching `tmux send-keys` usage.
pub fn send_prompt(session_id: &str, text: &str) -> Result<(), String> {
    let (workspace, handle) = resolve(&db()?, session_id)?;
    let text = format!("{text}\n");
    blocking(move |client| {
        let workspace = workspace.clone();
        let text = text.clone();
        async move { client.send_text(&workspace, handle, &text).await }
    })
}

/// Read the tail of a session's agent pane.
#[allow(dead_code)]
pub fn read_pane(session_id: &str, lines: usize) -> Result<String, String> {
    let captured = capture(session_id)?;
    Ok(planeai_core::capture_cursor::tail(&captured, lines))
}

/// Read output produced since `cursor`, using the capture-based strategy.
#[allow(dead_code)]
pub fn read_pane_after(
    session_id: &str,
    cursor: &str,
    max_bytes: usize,
) -> Result<planeai_core::capture_cursor::CursorRead, String> {
    let captured = capture(session_id)?;
    planeai_core::capture_cursor::read_after(CURSOR_LABEL, &captured, cursor, max_bytes)
}

/// Close a single resource, such as one shell tab.
///
/// The agent pane and sibling tabs keep running: each resource is its own window.
pub fn close_resource(pty_key: &str) -> Result<(), String> {
    let conn = db()?;
    let Some(record) = crate::rmux_resources::get(&conn, pty_key).map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let Some(workspace) = record.workspace() else {
        return Ok(());
    };
    let handle = record.handle();
    let result = blocking(move |client| {
        let workspace = workspace.clone();
        async move { client.close_resource(&workspace, handle).await }
    });
    // Forget the row either way: a pane PlaneAI cannot close is one it can no
    // longer address, and keeping the row would strand it.
    let _ = crate::rmux_resources::remove(&conn, pty_key);
    result
}

/// Close every resource of a session, leaving its workspace for siblings.
///
/// A workspace outlives the agents inside it and is removed only when its task is
/// deleted (ADR-0012), so archiving or destroying one agent must not take the
/// task's other agents with it.
pub fn close_session_resources(session_id: &str) -> Result<(), String> {
    let conn = db()?;
    let records = crate::rmux_resources::list_for_session(&conn, session_id)
        .map_err(|error| error.to_string())?;
    if records.is_empty() {
        return Ok(());
    }

    let closures: Vec<(WorkspaceName, ResourceHandle)> = records
        .iter()
        .filter_map(|record| record.workspace().map(|name| (name, record.handle())))
        .collect();

    let result = blocking(move |client| {
        let closures = closures.clone();
        async move {
            // Report the first failure but attempt every resource, so one stuck
            // pane cannot strand the rest.
            let mut first_error = None;
            for (workspace, handle) in closures {
                if let Err(error) = client.close_resource(&workspace, handle).await {
                    first_error = first_error.or(Some(error));
                }
            }
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    });

    // Forget the rows regardless: a pane we could not close is not one PlaneAI
    // can still address, and leaving the row would strand it forever.
    let _ = crate::rmux_resources::remove_for_session(&conn, session_id);
    result
}

/// Remove a task's whole workspace and every resource inside it.
#[allow(dead_code)]
pub fn delete_workspace(project_id: &str, task_key: &str) -> Result<(), String> {
    let workspace = WorkspaceKey::Task {
        project_id: project_id.to_string(),
        task_key: task_key.to_string(),
    }
    .name();
    blocking(move |client| {
        let workspace = workspace.clone();
        async move { client.kill_workspace(&workspace).await }
    })
}

/// Drop recorded panes the daemon no longer has, returning affected sessions.
pub fn prune_dead_resources(conn: &Connection) -> Result<Vec<String>, String> {
    // The caller owns the connection here because it also reconciles session rows.
    let live = blocking(|client| async move { client.live_pane_ids().await }).unwrap_or_default();
    crate::rmux_resources::prune_missing(conn, &live).map_err(|error| error.to_string())
}

#[allow(dead_code)]
fn capture(session_id: &str) -> Result<String, String> {
    let (workspace, handle) = resolve(&db()?, session_id)?;
    blocking(move |client| {
        let workspace = workspace.clone();
        async move { client.capture_resource_text(&workspace, handle).await }
    })
}

/// Resolve a session's agent resource to its workspace and pane.
fn resolve(conn: &Connection, session_id: &str) -> Result<(WorkspaceName, ResourceHandle), String> {
    let record = crate::rmux_resources::get(conn, session_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no rmux pane recorded for session {session_id}"))?;
    let workspace = record
        .workspace()
        .ok_or_else(|| format!("unrecognised rmux workspace: {}", record.workspace_key))?;
    Ok((workspace, record.handle()))
}
