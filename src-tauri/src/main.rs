#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod db;
mod notify;
mod pty;
mod tmux;

use std::sync::Mutex;
use std::sync::Arc;
use rusqlite::Connection;
use tauri::{State, Manager, menu::{Menu, Submenu, PredefinedMenuItem}};

struct DbState(Mutex<Connection>);
struct PtyState(pty::PtyManager);
struct NotifyHandle(notify::SharedNotifyState);
struct ConfigState(Mutex<config::Config>);

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return path.replacen("~", &home.to_string_lossy(), 1);
        }
    }
    path.to_string()
}

/// Check if a provider has hook-based idle detection (currently Kiro only).
fn provider_has_hook(provider_key: &str, cfg: &config::Config) -> bool {
    let Some(provider) = cfg.providers.get(provider_key) else { return false };
    provider.command.contains("kiro-cli") && is_notify_hook_installed_check()
}

fn is_notify_hook_installed_check() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{home}/.kiro/agents/default.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => content.contains("planeai-stop-notify"),
        Err(_) => false,
    }
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
fn list_sessions(state: State<DbState>, notify: State<NotifyHandle>, config_state: State<ConfigState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let sessions = db::list_sessions(&conn).map_err(|e| e.to_string())?;
    let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;

    // Register active sessions with notification system
    for s in &sessions {
        if s.status != "active" {
            continue;
        }
        let project_name = projects.iter()
            .find(|p| p.id == s.project_id)
            .map(|p| p.name.as_str())
            .unwrap_or("unknown");
        let display_name = if s.name.is_empty() { &s.branch } else { &s.name };
        let hook_enabled = s.provider.as_deref()
            .map(|pk| provider_has_hook(pk, &cfg))
            .unwrap_or(false);
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&s.id, display_name, project_name, hook_enabled);
    }

    Ok(sessions)
}

#[tauri::command]
fn rename_session(state: State<DbState>, notify: State<NotifyHandle>, config_state: State<ConfigState>, id: String, name: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::rename_session(&conn, &id, &name).map_err(|e| e.to_string())?;
    // Update notify display name
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
        let project_name = projects.iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.name.as_str())
            .unwrap_or("unknown");
        let display_name = if name.is_empty() { &session.branch } else { &name };
        let hook_enabled = session.provider.as_deref()
            .map(|pk| provider_has_hook(pk, &cfg))
            .unwrap_or(false);
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&id, display_name, project_name, hook_enabled);
    }
    Ok(())
}

#[tauri::command]
fn list_archived_sessions(state: State<DbState>) -> Result<Vec<db::Session>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_archived_sessions(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_session(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::restore_session(&conn, &id).map_err(|e| e.to_string())
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
fn get_config(state: State<ConfigState>) -> Result<config::Config, String> {
    let cfg = state.0.lock().map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

#[tauri::command]
fn update_config(state: State<ConfigState>, new_config: config::Config) -> Result<(), String> {
    let config_dir = config::config_dir();
    config::save(&config_dir, &new_config)?;
    let mut cfg = state.0.lock().map_err(|e| e.to_string())?;
    *cfg = new_config;
    Ok(())
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

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum AttachTarget {
    #[serde(rename = "tmux")]
    Tmux { tmux_name: String },
    #[serde(rename = "direct")]
    Direct { command: String, args: Vec<String>, cwd: String },
}

#[tauri::command]
fn attach_session(session_id: String, target: AttachTarget, state: State<PtyState>, app: tauri::AppHandle) -> Result<(), String> {
    let pty_target = match target {
        AttachTarget::Tmux { tmux_name } => pty::PtyTarget::TmuxAttach { tmux_name },
        AttachTarget::Direct { command, args, cwd } => pty::PtyTarget::Direct { command, args, cwd },
    };
    state.0.attach(&session_id, pty_target, app)
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
fn is_notify_hook_installed() -> bool {
    is_notify_hook_installed_check()
}

#[tauri::command]
fn install_notify_hook() -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;

    // Write the hook script
    let hooks_dir = format!("{home}/.kiro/hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;
    let script_path = format!("{hooks_dir}/planeai-stop-notify.sh");
    let script = include_str!("../resources/planeai-stop-notify.sh");
    std::fs::write(&script_path, script).map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    // Patch the default agent config to add our stop hook
    let agents_dir = format!("{home}/.kiro/agents");
    std::fs::create_dir_all(&agents_dir).map_err(|e| format!("failed to create agents dir: {e}"))?;
    let config_path = format!("{agents_dir}/default.json");

    let mut config: serde_json::Value = if let Ok(content) = std::fs::read_to_string(&config_path) {
        serde_json::from_str(&content).map_err(|e| format!("failed to parse default.json: {e}"))?
    } else {
        serde_json::json!({ "name": "default", "tools": ["*"] })
    };

    // Ensure hooks.stop array exists and add our entry
    let hooks = config.as_object_mut().unwrap()
        .entry("hooks").or_insert_with(|| serde_json::json!({}));
    let stop_hooks = hooks.as_object_mut().unwrap()
        .entry("stop").or_insert_with(|| serde_json::json!([]));
    let stop_arr = stop_hooks.as_array_mut().unwrap();

    // Check if already present
    let already = stop_arr.iter().any(|h| {
        h.get("command").and_then(|c| c.as_str()).map_or(false, |c| c.contains("planeai-stop-notify"))
    });
    if !already {
        stop_arr.push(serde_json::json!({
            "command": format!("{hooks_dir}/planeai-stop-notify.sh")
        }));
    }

    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, output).map_err(|e| format!("failed to write default.json: {e}"))?;
    Ok(())
}

#[tauri::command]
fn acknowledge_session(session_id: String, notify: State<NotifyHandle>) {
    let mut ns = notify.0.lock().unwrap();
    ns.acknowledge(&session_id);
}

#[tauri::command]
fn mark_exited(session_id: String, db_state: State<DbState>) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    db::mark_session_exited(&conn, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn check_tmux_available() -> bool {
    config::tmux_available()
}

#[tauri::command]
fn archive_session(id: String, db_state: State<DbState>, pty_state: State<PtyState>) -> Result<(), String> {
    pty_state.0.detach(&id);
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    db::archive_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn destroy_session(id: String, db_state: State<DbState>, pty_state: State<PtyState>) -> Result<(), String> {
    // Detach PTY
    pty_state.0.detach(&id);

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        // Only kill tmux if this is a tmux-backed session
        if session.backend == "tmux" {
            if let Some(ref tn) = session.tmux_name {
                let _ = tmux::kill_session(tn);
            }
        }
        // Remove worktree if applicable
        if let Some(ref wt_path) = session.worktree_path {
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
            if let Some(project) = projects.iter().find(|p| p.id == session.project_id) {
                let _ = tmux::worktree_remove(&project.path, wt_path);
            }
            let _ = std::fs::remove_dir_all(wt_path);
        }
    }
    // Soft-delete
    db::destroy_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn launch_session(
    state: State<DbState>,
    notify: State<NotifyHandle>,
    config_state: State<ConfigState>,
    project_id: String,
    project_name: String,
    repo_path: String,
    branch: String,
    is_new_branch: bool,
    name: String,
    use_worktree: bool,
    base_branch: Option<String>,
    auto_approve: bool,
    provider: Option<String>,
) -> Result<db::Session, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let provider_key = provider.unwrap_or_else(|| cfg.default_provider.clone());
    let provider_def = cfg.providers.get(&provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let cmd = config::launch_command(provider_def, auto_approve);
    let hook_enabled = provider_def.command.contains("kiro-cli") && is_notify_hook_installed_check();
    let backend = config::resolve_backend(&cfg).to_string();
    drop(cfg);

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
        tmux::checkout_branch(&repo_path, &branch, is_new_branch, base_branch.as_deref())?;
        (repo_path.clone(), None)
    };

    let session_id = uuid::Uuid::new_v4().to_string();

    let tmux_name = if backend == "tmux" {
        let tn = tmux::session_name(&project_name);
        tmux::create_session_with_cmd(&tn, &working_dir, &cmd, &session_id)?;
        Some(tn)
    } else {
        None
    };

    // Register with notification system
    {
        let mut ns = notify.0.lock().unwrap();
        let display_name = if name.is_empty() { &branch } else { &name };
        ns.register_session(&session_id, display_name, &project_name, hook_enabled);
    }

    // Persist to DB
    db::create_session_with_id(&conn, &session_id, &project_id, &name, tmux_name.as_deref(), &branch, worktree_path.as_deref(), Some(&provider_key), &backend)
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
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

            // Startup reconciliation: mark stale sessions as exited
            let _ = db::reconcile_sessions(&conn, |name| tmux::has_session(name));

            // Config: migrate from DB if needed, then load
            let config_dir = config::config_dir();
            if let Ok(settings) = db::get_settings(&conn) {
                let _ = config::migrate_from_db(&config_dir, &settings);
            }
            let (cfg, _warnings) = config::load(&config_dir);
            app.manage(ConfigState(Mutex::new(cfg)));

            app.manage(DbState(Mutex::new(conn)));

            // Notification system
            let notify_state: notify::SharedNotifyState = Arc::new(Mutex::new(notify::NotifyState::new()));
            notify::start_socket_listener(&app_dir, notify_state.clone(), app.handle().clone());
            notify::start_silence_checker(notify_state.clone(), app.handle().clone());
            app.manage(NotifyHandle(notify_state.clone()));

            // PTY manager with notify wired in
            let pty_mgr = pty::PtyManager::new();
            pty_mgr.set_notify_state(notify_state);
            app.manage(PtyState(pty_mgr));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            list_projects,
            delete_project,
            create_session,
            list_sessions,
            delete_session,
            rename_session,
            list_archived_sessions,
            restore_session,
            validate_git_repo,
            list_branches,
            list_monospace_fonts,
            get_config,
            update_config,
            launch_session,
            attach_session,
            write_to_pty,
            resize_pty,
            check_session_alive,
            is_notify_hook_installed,
            install_notify_hook,
            acknowledge_session,
            mark_exited,
            check_tmux_available,
            archive_session,
            destroy_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
