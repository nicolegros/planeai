use tauri::State;

use crate::state::DbState;
use planeai_core::cleanup::StaleWorktree;

#[tauri::command]
pub fn list_stale_worktrees(state: State<DbState>) -> Result<Vec<StaleWorktree>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let items = planeai_core::cleanup::list_stale_worktrees(&conn)?;
    tracing::info!("manual cleanup: found {} stale worktree(s)", items.len());
    Ok(items)
}

#[tauri::command]
pub fn run_stale_worktree_cleanup(state: State<DbState>) -> Result<Vec<String>, String> {
    tracing::info!("manual cleanup: starting");
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let errors =
        planeai_core::cleanup::run_stale_worktree_cleanup(&conn, |project_path, wt_path| {
            if !std::path::Path::new(wt_path).exists() {
                tracing::debug!("manual cleanup: skipping non-existent {wt_path}");
                return Ok(());
            }
            tracing::info!("manual cleanup: removing {wt_path}");
            planeai_core::git::worktree_remove(project_path, wt_path)?;
            if std::path::Path::new(wt_path).exists() {
                std::fs::remove_dir_all(wt_path).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
    for e in &errors {
        tracing::warn!("manual cleanup: {e}");
    }
    tracing::info!("manual cleanup: complete ({} error(s))", errors.len());
    Ok(errors)
}
