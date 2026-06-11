use tauri::{Emitter, State};

use crate::cleanup;
use crate::config;
use crate::db;
use crate::state::{ConfigState, DbState, PtyState};
#[cfg(not(windows))]
use crate::tmux;

use super::helpers::{fire_task_hook, session_cwd};

#[tauri::command]
pub fn restart_session(
    session_id: String,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
) -> Result<db::Session, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    if session.status != "exited" {
        return Err("can only restart exited sessions".to_string());
    }

    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let provider_key = session.provider.as_deref().unwrap_or(&cfg.default_provider);
    let provider_def = cfg
        .providers
        .get(provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let has_resume = session.provider_session_id.is_some() && provider_def.resume_flag.is_some();
    let _cmd = if has_resume {
        config::restart_command_for_provider(provider_def, session.provider_session_id.as_deref())
    } else {
        config::launch_command(provider_def, session.auto_approve)
    };
    drop(cfg);

    if session.backend == "tmux" {
        #[cfg(not(windows))]
        {
            let tmux_name = session
                .tmux_name
                .as_deref()
                .ok_or("tmux session has no tmux_name")?;
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
            let project_path = projects
                .iter()
                .find(|p| p.id == session.project_id)
                .map(|p| p.path.as_str())
                .unwrap_or("/");
            let cwd = session.worktree_path.as_deref().unwrap_or(project_path);
            tmux::create_session_with_cmd(tmux_name, cwd, &_cmd, &session_id)?;
        }
        #[cfg(windows)]
        return Err("tmux backend not available on Windows".to_string());
    }

    db::restore_session(&conn, &session_id).map_err(|e| e.to_string())?;
    let updated = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found after restore")?;

    if updated.task_key.is_some() {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        if let Some(cwd) = session_cwd(&conn, &updated) {
            fire_task_hook(&cfg, &updated, "on_restart", &cwd);
        }
    }

    Ok(updated)
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
    crate::session_ops::archive(&conn, &id, &Some(cfg))?;
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

    let cleanup_ctx;
    let task_hook_ctx;
    {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let session = db::get_session(&conn, &id).map_err(|e| e.to_string())?;

        if let Some(ref session) = session {
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
            let project_path = projects
                .iter()
                .find(|p| p.id == session.project_id)
                .map(|p| p.path.clone());

            cleanup_ctx = Some(cleanup::CleanupContext {
                backend: session.backend.clone(),
                tmux_name: session.tmux_name.clone(),
                worktree_path: session.worktree_path.clone(),
                project_path: project_path.clone(),
                branch: if session.worktree_path.is_some() {
                    Some(session.branch.clone())
                } else {
                    None
                },
            });

            task_hook_ctx = if session.task_key.is_some() {
                let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
                let cwd = session_cwd(&conn, session);
                Some((cfg.clone(), session.clone(), cwd))
            } else {
                None
            };
        } else {
            cleanup_ctx = None;
            task_hook_ctx = None;
        }

        db::destroy_session(&conn, &id).map_err(|e| e.to_string())?;
    }

    if let Some(ctx) = cleanup_ctx {
        std::thread::spawn(move || {
            if let Some((cfg, session, Some(cwd))) = task_hook_ctx {
                fire_task_hook(&cfg, &session, "on_complete", &cwd);
            }

            let errors = cleanup::run_cleanup(&ctx, &cleanup::real_ops());

            if !errors.is_empty() {
                let msg = errors.join("; ");
                let _ = app_handle.emit("cleanup-error", msg);
            }
        });
    }

    Ok(())
}
