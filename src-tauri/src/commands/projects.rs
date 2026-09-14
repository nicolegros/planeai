use tauri::State;

use crate::cleanup;
use crate::commands::sessions::lifecycle::session_lifecycle_event;
use crate::db;
use crate::git;
use crate::plugins::PluginRuntimeHandle;
use crate::state::{DbState, ProjectOperationState, PtyState};
use crate::util::expand_tilde;

fn resolve_project_path(path: &str) -> Result<String, String> {
    std::fs::canonicalize(expand_tilde(path))
        .map_err(|error| format!("Cannot resolve project path: {error}"))?
        .into_os_string()
        .into_string()
        .map_err(|_| "Project path is not valid UTF-8.".to_string())
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, DbState>,
    name: String,
    path: String,
) -> Result<db::Project, String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let path = resolve_project_path(&path)?;
        let conn = conn.lock().map_err(|e| e.to_string())?;
        if db::project_name_exists(&conn, &name).map_err(|e| e.to_string())? {
            return Err(format!("A project named '{}' already exists.", name));
        }
        if db::project_path_in_use(&conn, &path, "").map_err(|e| e.to_string())? {
            return Err(format!("A project already uses the path '{}'.", path));
        }
        db::create_project(&conn, &name, &path).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn update_project(
    state: State<'_, DbState>,
    operations: State<'_, ProjectOperationState>,
    id: String,
    name: String,
    path: String,
) -> Result<db::Project, String> {
    let operation_lock = operations.lock_for(&id);
    let _operation_guard = operation_lock.lock_owned().await;
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let path = resolve_project_path(&path)?;
        let conn = conn.lock().map_err(|e| e.to_string())?;
        let current = db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Project not found.".to_string())?;
        if current.name != name
            && db::project_name_exists(&conn, &name).map_err(|e| e.to_string())?
        {
            return Err(format!("A project named '{}' already exists.", name));
        }
        if current.path != path {
            if db::project_path_in_use(&conn, &path, &id).map_err(|e| e.to_string())? {
                return Err(format!("A project already uses the path '{}'.", path));
            }
            if db::project_has_worktree_sessions(&conn, &id).map_err(|e| e.to_string())? {
                return Err(
                    "Cannot change a project path while it has worktree sessions. Remove those sessions first."
                        .to_string(),
                );
            }
        }
        db::update_project(&conn, &id, &name, &path).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn list_projects(state: State<DbState>) -> Result<Vec<db::Project>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_archived_projects(state: State<DbState>) -> Result<Vec<db::Project>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_archived_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_project(
    state: State<DbState>,
    runtime: State<PluginRuntimeHandle>,
    id: String,
) -> Result<(), String> {
    let lifecycle_events = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let sessions = db::get_project_sessions(&conn, &id).map_err(|e| e.to_string())?;
        db::archive_project(&conn, &id).map_err(|e| e.to_string())?;
        sessions
            .into_iter()
            .filter(|session| session.status != "archived")
            .map(|session| session_lifecycle_event(&session, &session.status, "archived"))
            .collect::<Vec<_>>()
    };
    for event in lifecycle_events {
        runtime.0.dispatch_session_lifecycle(event);
    }
    Ok(())
}

#[tauri::command]
pub async fn restore_project(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let archived_path = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Project not found.".to_string())?
            .path
    };
    let path = crate::commands::blocking(move || resolve_project_path(&archived_path)).await?;
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        let project = db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Project not found.".to_string())?;
        if db::project_path_in_use(&conn, &path, &id).map_err(|e| e.to_string())? {
            return Err(format!("A project already uses the path '{}'.", path));
        }
        db::update_project(&conn, &id, &project.name, &path).map_err(|e| e.to_string())?;
        db::restore_project(&conn, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn hide_project(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        db::hide_project(&conn, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn unhide_project(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = state.0.clone();
    crate::commands::blocking(move || {
        let conn = conn.lock().map_err(|e| e.to_string())?;
        db::unhide_project(&conn, &id).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn get_project_auto_mode(state: State<DbState>, id: String) -> Result<bool, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT auto_mode FROM projects WHERE id = ?1",
        [&id],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_project_auto_mode(
    state: State<DbState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE projects SET auto_mode = ?1 WHERE id = ?2",
        rusqlite::params![enabled as i64, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_project(
    state: State<'_, DbState>,
    pty_state: State<'_, PtyState>,
    runtime: State<'_, PluginRuntimeHandle>,
    operations: State<'_, ProjectOperationState>,
    id: String,
) -> Result<(), String> {
    // Serialize deletion with launches and PTY attaches for this project. The guard
    // remains held through teardown so a local-backend process cannot appear after
    // the initial PTY detach.
    let operation_lock = operations.lock_for(&id);
    let _operation_guard = operation_lock.lock_owned().await;

    let (sessions, project_path, owned_worktrees) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let sessions = db::get_project_sessions(&conn, &id).map_err(|e| e.to_string())?;
        let project_path = db::get_project(&conn, &id)
            .map_err(|e| e.to_string())?
            .map(|project| project.path);
        let owned_worktrees = sessions
            .iter()
            .map(|session| db::session_owns_worktree(&conn, &session.id).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        (sessions, project_path, owned_worktrees)
    };

    // PtyState is main-thread-only; detach before moving process and filesystem
    // teardown to the blocking worker.
    for session in &sessions {
        pty_state.0.detach(&session.id);
    }

    let teardown_sessions = sessions.clone();
    crate::commands::blocking(move || {
        for (session, owns_worktree) in teardown_sessions.iter().zip(owned_worktrees) {
            let kill_errors = cleanup::kill_backend(
                &session.backend,
                session.tmux_name.as_deref(),
                Some(&session.id),
                session.tab_count,
                &cleanup::real_kill_ops(),
            );
            if !kill_errors.is_empty() {
                tracing::warn!(session_id = %session.id, ?kill_errors, "failed to fully stop backend while deleting project");
            }
            if owns_worktree {
                if let (Some(project_path), Some(worktree_path)) =
                    (project_path.as_deref(), session.worktree_path.as_deref())
                {
                    let _ = git::worktree_remove(project_path, worktree_path);
                    let _ = std::fs::remove_dir_all(worktree_path);
                }
            }
        }
        Ok(())
    })
    .await?;

    let lifecycle_events = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        for session in &sessions {
            if session.status != "destroyed" {
                db::destroy_session(&conn, &session.id).map_err(|e| e.to_string())?;
            }
        }
        db::delete_project(&conn, &id).map_err(|e| e.to_string())?;
        sessions
            .iter()
            .filter(|session| session.status != "destroyed")
            .map(|session| session_lifecycle_event(session, &session.status, "destroyed"))
            .collect::<Vec<_>>()
    };
    for event in lifecycle_events {
        runtime.0.dispatch_session_lifecycle(event);
    }
    Ok(())
}
