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
    // Reuse is the client's job: `spawn_resource` correlates on the window name
    // and adopts a pane that already hosts this `pty_key`. Checking the recorded
    // pane id here instead would trust the database over the daemon, and would
    // miss exactly the case that matters — a pane created by a spawn whose reply
    // was lost, which has no record at all.
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
    // A failed write closes the pane: it is running but unreferenced, and no sweep
    // keyed on `rmux_resources` would ever find it again.
    let rollback_workspace = workspace.clone();
    persist_or_close(
        pty_key,
        || {
            crate::rmux_resources::put(&db()?, pty_key, session_id, workspace, handle)
                .map_err(|error| error.to_string())
        },
        || {
            blocking(move |client| {
                let workspace = rollback_workspace.clone();
                async move { client.close_resource(&workspace, handle).await }
            })
        },
    )?;
    Ok(handle)
}

/// Send a prompt to a session's agent pane and submit it.
///
/// `send_text` is the literal `send-keys -l` path, so the text reaches the tty
/// unmodified. The submit byte is the client's concern — agents read raw-mode
/// Enter as `\r`, not `\n`.
pub fn send_prompt(session_id: &str, text: &str) -> Result<(), String> {
    let (workspace, handle) = resolve(&db()?, session_id)?;
    let text = text.to_string();
    blocking(move |client| {
        let workspace = workspace.clone();
        let text = text.clone();
        async move { client.submit_text(&workspace, handle, &text).await }
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

/// Tear down a task's whole workspace: rmux session, metadata, and mappings.
///
/// ADR-0012 specifies this as an explicit, idempotent, crash-safe sequence —
/// delete the rmux session, then the workspace metadata, then the task — because
/// `task_workspaces` cannot foreign-key to tasks held in task-manager storage, so
/// nothing cascades. Running it twice is harmless, which is what makes a crash
/// between the steps recoverable: the next delete finishes the job.
///
/// Killing the session is best effort. A task must stay deletable with no rmux
/// daemon running, which is the normal case for every other backend, and a pane
/// left behind by an unreachable daemon is collected by the orphan sweep. The
/// local deletions are not best effort: they cannot fail for want of a daemon, so
/// a failure there is real and is reported rather than leaving rows behind a task
/// that no longer exists.
// Reached from the library by `task_cli`; the Tauri binary compiles its own copy
// of this module where that caller is absent.
#[allow(dead_code)]
pub fn delete_workspace(project_id: &str, task_key: &str) -> Result<(), String> {
    let workspace = WorkspaceKey::Task {
        project_id: project_id.to_string(),
        task_key: task_key.to_string(),
    }
    .name();

    let kill = {
        let workspace = workspace.clone();
        blocking(move |client| {
            let workspace = workspace.clone();
            async move { client.kill_workspace(&workspace).await }
        })
    };
    if let Err(error) = &kill {
        tracing::debug!(task_key, %error, "no rmux workspace killed for task");
    }

    let conn = db()?;
    conn.execute(
        "DELETE FROM task_workspaces WHERE project_id = ?1 AND task_key = ?2",
        rusqlite::params![project_id, task_key],
    )
    .map_err(|error| error.to_string())?;
    let forgotten = crate::rmux_resources::remove_for_workspace(&conn, &workspace)
        .map_err(|error| error.to_string())?;
    tracing::info!(
        task_key,
        forgotten,
        killed = kill.is_ok(),
        "removed task workspace"
    );
    Ok(())
}

/// Drop recorded panes the daemon no longer has, returning affected sessions.
pub fn prune_dead_resources(conn: &Connection) -> Result<Vec<String>, String> {
    // The caller owns the connection here because it also reconciles session rows.
    let live = blocking(|client| async move { client.live_pane_ids().await }).unwrap_or_default();
    crate::rmux_resources::prune_missing(conn, &live).map_err(|error| error.to_string())
}

/// Live panes PlaneAI has no session for, as a proposal rather than an action.
///
/// Separated from [`sweep_orphan_panes`] so the decision can be taken twice and
/// compared before anything is closed.
pub fn orphan_candidates(
    conn: &Connection,
    known_session_ids: &std::collections::HashSet<String>,
) -> Result<crate::rmux_resources::OrphanSweep, String> {
    // An unreachable daemon hosts nothing, so there is nothing to propose. Records
    // are left alone: `prune_dead_resources` owns that direction and can tell a
    // dead daemon from a missing pane.
    let Ok(live) = blocking(|client| async move { client.live_panes().await }) else {
        return Ok(crate::rmux_resources::OrphanSweep::default());
    };
    crate::rmux_resources::orphaned_panes(conn, &live, known_session_ids)
        .map_err(|error| error.to_string())
}

/// Close the given panes and forget the given records. Returns how many closed.
///
/// The counterpart to [`prune_dead_resources`], which only handles the other
/// direction: that drops records whose pane died, this closes panes that outlived
/// their record.
pub fn sweep_orphan_panes(
    conn: &Connection,
    sweep: &crate::rmux_resources::OrphanSweep,
) -> Result<usize, String> {
    if sweep.is_empty() {
        return Ok(0);
    }

    let mut closures: Vec<(WorkspaceName, ResourceHandle)> = Vec::new();
    for pane in &sweep.panes {
        // A name PlaneAI cannot parse is not one it should be killing.
        let Some(workspace) = WorkspaceName::from_stored(&pane.workspace_key) else {
            tracing::warn!(
                workspace_key = %pane.workspace_key,
                pane_id = pane.pane_id,
                "skipping orphan sweep for an unrecognised rmux workspace"
            );
            continue;
        };
        tracing::info!(
            workspace_key = %pane.workspace_key,
            pane_id = pane.pane_id,
            "closing orphaned rmux pane: no PlaneAI session owns it"
        );
        closures.push((workspace, ResourceHandle::from_u32(pane.pane_id)));
    }

    let closed = closures.len();
    if !closures.is_empty() {
        // Attempt every pane: one stuck pane must not strand the rest.
        let result = blocking(move |client| {
            let closures = closures.clone();
            async move {
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
        if let Err(error) = result {
            tracing::warn!(%error, "could not close every orphaned rmux pane");
        }
    }

    for pty_key in &sweep.stale_records {
        let _ = crate::rmux_resources::remove(conn, pty_key);
    }
    Ok(closed)
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

/// Persist a spawned pane's mapping, closing the pane if that fails.
///
/// The spawn has already succeeded by the time this runs, so a failed write would
/// otherwise leave a live pane that nothing references: the launch reports an
/// error, no row points at the pane, and it survives every later sweep keyed on
/// `rmux_resources`. Closing it keeps the failure atomic.
///
/// The persistence error is what the caller sees. A rollback that also fails is
/// logged rather than returned — it is a consequence of the first failure, and
/// replacing the cause with it would hide why the launch failed.
fn persist_or_close<Persist, Close>(
    pty_key: &str,
    persist: Persist,
    close: Close,
) -> Result<(), String>
where
    Persist: FnOnce() -> Result<(), String>,
    Close: FnOnce() -> Result<(), String>,
{
    let Err(error) = persist() else {
        return Ok(());
    };
    tracing::warn!(
        pty_key,
        %error,
        "could not record rmux pane; closing it so the launch leaves no orphan"
    );
    if let Err(rollback_error) = close() {
        tracing::error!(
            pty_key,
            %rollback_error,
            "orphaned rmux pane: recording failed and the pane could not be closed"
        );
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_recorded_pane_is_left_running() {
        let closed = Cell::new(false);

        let result = persist_or_close(
            "session-a",
            || Ok(()),
            || {
                closed.set(true);
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert!(
            !closed.get(),
            "a successfully recorded pane must keep running"
        );
    }

    #[test]
    fn a_pane_that_could_not_be_recorded_is_closed() {
        let closed = Cell::new(false);

        let result = persist_or_close(
            "session-a",
            || Err("database is locked".to_string()),
            || {
                closed.set(true);
                Ok(())
            },
        );

        // The caller must see why the launch failed, not how it was cleaned up.
        assert_eq!(result, Err("database is locked".to_string()));
        assert!(closed.get(), "an unrecorded pane must not be left running");
    }

    #[test]
    fn a_failed_rollback_still_reports_the_original_failure() {
        let result = persist_or_close(
            "session-a",
            || Err("database is locked".to_string()),
            || Err("daemon is unreachable".to_string()),
        );

        assert_eq!(result, Err("database is locked".to_string()));
    }
}
