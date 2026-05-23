#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod pty;
mod tmux;

use std::sync::Mutex;
use rusqlite::Connection;
use tauri::{State, Manager, menu::{Menu, Submenu, PredefinedMenuItem}};

struct DbState(Mutex<Connection>);
struct PtyState(pty::PtyManager);

#[tauri::command]
fn create_project(state: State<DbState>, name: String, path: String) -> Result<db::Project, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
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
fn create_session(state: State<DbState>, project_id: String, tmux_name: String, branch: String) -> Result<db::Session, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::create_session(&conn, &project_id, &tmux_name, &branch).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_sessions(state: State<DbState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let sessions = db::list_sessions(&conn).map_err(|e| e.to_string())?;
    let mut alive = Vec::new();
    for s in sessions {
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
    let git_dir = std::path::Path::new(&path).join(".git");
    Ok(git_dir.exists())
}

#[tauri::command]
fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    tmux::list_branches(&repo_path)
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
fn destroy_session(id: String, tmux_name: String, db_state: State<DbState>, pty_state: State<PtyState>) -> Result<(), String> {
    // Kill tmux session
    tmux::kill_session(&tmux_name)?;
    // Detach PTY
    pty_state.0.detach(&id);
    // Remove from DB
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
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
) -> Result<db::Session, String> {
    // Checkout branch
    tmux::checkout_branch(&repo_path, &branch, is_new_branch)?;

    // Create tmux session
    let tmux_name = tmux::session_name(&project_name);
    tmux::create_session(&tmux_name, &repo_path)?;

    // Persist to DB
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::create_session(&conn, &project_id, &tmux_name, &branch).map_err(|e| e.to_string())
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

            app.get_webview_window("main").unwrap().open_devtools();

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
            launch_session,
            attach_session,
            write_to_pty,
            resize_pty,
            check_session_alive,
            destroy_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
