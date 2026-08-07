use tauri::State;

use crate::db;
use crate::git;
use crate::state::{DbState, PtyState};
#[cfg(not(windows))]
use crate::tmux;
use crate::util::expand_tilde;

#[tauri::command]
pub fn create_project(
    state: State<DbState>,
    name: String,
    path: String,
) -> Result<db::Project, String> {
    let path = expand_tilde(&path);
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if db::project_name_exists(&conn, &name).map_err(|e| e.to_string())? {
        return Err(format!("A project named '{}' already exists.", name));
    }
    db::create_project(&conn, &name, &path).map_err(|e| e.to_string())
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
pub fn archive_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::archive_project(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::restore_project(&conn, &id).map_err(|e| e.to_string())
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
pub fn delete_project(
    state: State<DbState>,
    pty_state: State<PtyState>,
    id: String,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let sessions = db::get_project_sessions(&conn, &id).map_err(|e| e.to_string())?;
    let project = db::get_project(&conn, &id).map_err(|e| e.to_string())?;
    for session in &sessions {
        pty_state.0.detach(&session.id);
        if session.backend == "tmux" {
            #[cfg(not(windows))]
            if let Some(ref tn) = session.tmux_name {
                let _ = tmux::kill_session(tn);
            }
        }
        if let Some(ref wt_path) = session.worktree_path {
            if let Some(ref proj) = project {
                let _ = git::worktree_remove(&proj.path, wt_path);
            }
            let _ = std::fs::remove_dir_all(wt_path);
        }
    }
    db::delete_project(&conn, &id).map_err(|e| e.to_string())
}
