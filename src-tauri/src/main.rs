#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod pty;
mod tmux;

use std::sync::Mutex;
use rusqlite::Connection;
use tauri::{State, Manager, menu::{Menu, Submenu, PredefinedMenuItem}};

struct DbState(Mutex<Connection>);
struct PtyState(pty::PtyManager);

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return path.replacen("~", &home.to_string_lossy(), 1);
        }
    }
    path.to_string()
}

#[tauri::command]
fn create_project(state: State<DbState>, name: String, path: String) -> Result<db::Project, String> {
    let path = expand_tilde(&path);
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if db::project_name_exists(&conn, &name).map_err(|e| e.to_string())? {
        return Err(format!("A project named '{}' already exists.", name));
    }
    db::create_project(&conn, &name, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_projects(state: State<DbState>) -> Result<Vec<db::Project>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::delete_project(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_session(state: State<DbState>, project_id: String, name: String, tmux_name: String, branch: String) -> Result<db::Session, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::create_session(&conn, &project_id, &name, &tmux_name, &branch, None).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_sessions(state: State<DbState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let sessions = db::list_sessions(&conn).map_err(|e| e.to_string())?;
    let mut alive = Vec::new();
    for s in sessions {
        if s.status == "archived" {
            continue;
        }
        if tmux::has_session(&s.tmux_name) {
            alive.push(s);
        } else {
            let _ = db::delete_session(&conn, &s.id);
        }
    }
    Ok(alive)
}

#[tauri::command]
fn delete_session(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::delete_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn validate_git_repo(path: String) -> Result<bool, String> {
    let path = expand_tilde(&path);
    let git_dir = std::path::Path::new(&path).join(".git");
    Ok(git_dir.exists())
}

#[tauri::command]
fn get_settings(state: State<DbState>) -> Result<db::Settings, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::get_settings(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_settings(state: State<DbState>, settings: db::Settings) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::update_settings(&conn, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    tmux::list_branches(&repo_path)
}

#[tauri::command]
fn list_monospace_fonts() -> Result<Vec<String>, String> {
    let output = std::process::Command::new("fc-list")
        .args([":spacing=100", "family"])
        .output()
        .map_err(|e| format!("failed to run fc-list: {e}"))?;

    if !output.status.success() {
        return Err("fc-list failed".to_string());
    }

    let mut fonts: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.split(',').next().unwrap_or("").trim().to_string())
        .filter(|f| !f.is_empty() && !f.starts_with('.'))
        .collect();
    fonts.sort();
    fonts.dedup();
    Ok(fonts)
}

#[tauri::command]
fn attach_session(session_id: String, tmux_name: String, state: State<PtyState>, app: tauri::AppHandle) -> Result<(), String> {
    state.0.attach(&session_id, &tmux_name, app)
}

#[tauri::command]
fn write_to_pty(session_id: String, data: Vec<u8>, state: State<PtyState>) -> Result<(), String> {
    state.0.write(&session_id, &data)
}

#[tauri::command]
fn resize_pty(session_id: String, rows: u16, cols: u16, state: State<PtyState>) -> Result<(), String> {
    state.0.resize(&session_id, rows, cols)
}

#[tauri::command]
fn check_session_alive(tmux_name: String) -> bool {
    tmux::has_session(&tmux_name)
}

#[tauri::command]
fn archive_session(id: String, tmux_name: String, db_state: State<DbState>, pty_state: State<PtyState>) -> Result<(), String> {
    tmux::kill_session(&tmux_name)?;
    pty_state.0.detach(&id);
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    db::archive_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn destroy_session(id: String, tmux_name: String, db_state: State<DbState>, pty_state: State<PtyState>) -> Result<(), String> {
    // Kill tmux session
    tmux::kill_session(&tmux_name)?;
    // Detach PTY
    pty_state.0.detach(&id);
    // Remove worktree if applicable
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        if let Some(ref wt_path) = session.worktree_path {
            // Find project repo path for git worktree remove
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
            if let Some(project) = projects.iter().find(|p| p.id == session.project_id) {
                let _ = tmux::worktree_remove(&project.path, wt_path);
            }
            // Clean up directory if it still exists
            let _ = std::fs::remove_dir_all(wt_path);
        }
    }
    // Remove from DB
    db::delete_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn launch_session(
    state: State<DbState>,
    project_id: String,
    project_name: String,
    repo_path: String,
    branch: String,
    is_new_branch: bool,
    name: String,
    use_worktree: bool,
    base_branch: Option<String>,
    auto_approve: bool,
) -> Result<db::Session, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    let (working_dir, worktree_path) = if use_worktree {
        let base = base_branch.as_deref().unwrap_or("main");
        let session_id = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
        let sanitized_project = project_name.replace(' ', "-").to_lowercase();
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let wt_path = format!("{home}/.planeai/worktrees/{sanitized_project}/{session_id}");
        std::fs::create_dir_all(std::path::Path::new(&wt_path).parent().unwrap())
            .map_err(|e| format!("failed to create worktree dir: {e}"))?;
        tmux::worktree_add(&repo_path, &wt_path, &branch, base)?;
        (wt_path.clone(), Some(wt_path))
    } else {
        // Guard: block if another non-worktree session is active for this project
        if db::has_active_checkout_session(&conn, &project_id).map_err(|e| e.to_string())? {
            return Err("Another in-repo session is already active for this project. Archive it first or use worktree mode.".to_string());
        }
        tmux::checkout_branch(&repo_path, &branch, is_new_branch)?;
        (repo_path, None)
    };

    // Create tmux session
    let tmux_name = tmux::session_name(&project_name);
    tmux::create_session(&tmux_name, &working_dir, auto_approve)?;

    // Persist to DB
    db::create_session(&conn, &project_id, &name, &tmux_name, &branch, worktree_path.as_deref())
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let menu = Menu::with_items(app, &[
                &Submenu::with_items(app, "planeai", true, &[
                    &PredefinedMenuItem::about(app, None, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::show_all(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ])?,
                &Submenu::with_items(app, "Edit", true, &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ])?,
                &Submenu::with_items(app, "Window", true, &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                    &PredefinedMenuItem::close_window(app, None)?,
                    &PredefinedMenuItem::fullscreen(app, None)?,
                ])?,
            ])?;
            app.set_menu(menu)?;

            let app_dir = app.path().app_data_dir().expect("failed to get app data dir");
            std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");
            let db_path = app_dir.join("planeai.db");
            let conn = Connection::open(db_path).expect("failed to open database");
            db::migrate(&conn).expect("failed to run migrations");
            app.manage(DbState(Mutex::new(conn)));
            app.manage(PtyState(pty::PtyManager::new()));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            list_projects,
            delete_project,
            create_session,
            list_sessions,
            delete_session,
            validate_git_repo,
            list_branches,
            list_monospace_fonts,
            get_settings,
            update_settings,
            launch_session,
            attach_session,
            write_to_pty,
            resize_pty,
            check_session_alive,
            archive_session,
            destroy_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
