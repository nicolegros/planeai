#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod command;
mod config;
mod db;
mod git;
mod notify;
mod pty;
mod task_manager;
mod template;
#[cfg(not(windows))]
mod tmux;

use std::sync::Mutex;
use std::sync::Arc;
use rusqlite::Connection;
use tauri::{State, Manager, Emitter, menu::{Menu, Submenu, PredefinedMenuItem}};
use tauri::ipc::Channel;

struct DbState(Mutex<Connection>);
struct PtyState(pty::PtyManager);
struct NotifyHandle(notify::SharedNotifyState);
struct ConfigState(Mutex<config::Config>);

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        let home = config::home_dir();
        if !home.is_empty() {
            return path.replacen("~", &home, 1);
        }
    }
    path.to_string()
}

/// Shell-escape a string by wrapping in single quotes, escaping any internal single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Resolve a command name to its full path, checking user-local bin directories
/// that may not be in PATH when launched from a GUI app.
fn resolve_command(cmd: &str) -> String {
    command::resolve(cmd)
}

/// Background discovery of provider session ID with retry-backoff.
/// Runs list_sessions_command in the session's cwd, parses the output, and stores the result.
fn discover_provider_session_id(
    session_id: &str,
    list_cmd: &str,
    pattern: &str,
    cwd: &str,
    previous_id: Option<&str>,
    is_resume: bool,
    db_path: &std::path::Path,
    app: &tauri::AppHandle,
) {
    let delays = [1, 2, 4];
    let mut last_discovered: Option<String> = None;
    for delay in &delays {
        std::thread::sleep(std::time::Duration::from_secs(*delay));
        let parts: Vec<&str> = list_cmd.split_whitespace().collect();
        let resolved = resolve_command(parts[0]);
        eprintln!("[DEBUG-disc] attempt after {delay}s: running '{resolved}' with args {:?} in cwd '{cwd}'", &parts[1..]);
        let output = std::process::Command::new(&resolved)
            .args(&parts[1..])
            .current_dir(cwd)
            .output();
        match &output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("[DEBUG-disc] exit={}, stdout_len={}, stderr_len={}", o.status, stdout.len(), stderr.len());
                if !o.status.success() { continue; }
                // Try stdout first, then stderr (some providers write to stderr)
                let combined = if stdout.is_empty() { stderr.to_string() } else { stdout.to_string() };
                let discovered = config::parse_provider_session_id(&combined, pattern);
                eprintln!("[DEBUG-disc] parsed session_id={:?}, previous={:?}, is_resume={}", discovered, previous_id, is_resume);
                if config::should_accept_provider_session_id(discovered.as_deref(), previous_id, is_resume) {
                    eprintln!("[DEBUG-disc] accepted! storing provider_session_id={:?}", discovered);
                    if let Ok(conn) = rusqlite::Connection::open(db_path) {
                        let _ = db::set_provider_session_id(&conn, session_id, discovered.as_ref().unwrap());
                    }
                    return;
                } else {
                    eprintln!("[DEBUG-disc] rejected (stale or no match)");
                    last_discovered = discovered;
                }
            }
            Err(e) => {
                eprintln!("[DEBUG-disc] command failed to execute: {e}");
                continue;
            }
        }
    }
    // If resume-based discovery failed but we found a new session ID, accept it.
    // This handles the case where the old session is gone (e.g. computer restarted)
    // and the provider started a fresh session with a new ID.
    if is_resume {
        if let Some(new_id) = last_discovered {
            eprintln!("[DEBUG-disc] resume failed, accepting new session id={}", new_id);
            if let Ok(conn) = rusqlite::Connection::open(db_path) {
                let _ = db::set_provider_session_id(&conn, session_id, &new_id);
            }
            return;
        }
    }
    // Discovery failed — emit event to notify frontend
    let _ = app.emit("provider-session-id-failed", serde_json::json!({
        "session_id": session_id,
        "reason": "Could not discover provider session ID after retries"
    }));
}

/// Check if a provider has hook-based idle detection.
fn provider_has_hook(provider_key: &str, cfg: &config::Config) -> bool {
    let Some(provider) = cfg.providers.get(provider_key) else { return false };
    if provider.command.contains("kiro") {
        is_kiro_hook_installed()
    } else if provider.command.contains("claude") {
        is_claude_hook_installed()
    } else {
        false
    }
}

fn is_kiro_hook_installed() -> bool {
    let home = config::home_dir();
    let path = format!("{home}/.kiro/agents/default.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => content.contains("planeai-stop-notify"),
        Err(_) => false,
    }
}

fn is_claude_hook_installed() -> bool {
    let home = config::home_dir();
    let path = std::path::PathBuf::from(format!("{home}/.claude/settings.json"));
    notify::is_claude_hook_installed_at(&path)
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
fn list_archived_projects(state: State<DbState>) -> Result<Vec<db::Project>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_archived_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn archive_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::archive_project(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::restore_project(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_project(state: State<DbState>, pty_state: State<PtyState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let sessions = db::get_project_sessions(&conn, &id).map_err(|e| e.to_string())?;
    let project = db::get_project(&conn, &id).map_err(|e| e.to_string())?;
    // Kill tmux sessions and remove worktrees
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
fn update_config(state: State<ConfigState>, new_config: config::Config, app: tauri::AppHandle) -> Result<(), String> {
    let config_dir = config::config_dir(&app.package_info().name);
    config::save(&config_dir, &new_config)?;
    let mut cfg = state.0.lock().map_err(|e| e.to_string())?;
    *cfg = new_config;
    Ok(())
}

#[tauri::command]
fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    git::list_branches(&repo_path)
}

#[tauri::command]
fn get_changed_files(repo_path: String, base_branch: String) -> Result<Vec<git::ChangedFile>, String> {
    git::get_changed_files(&repo_path, &base_branch)
}

#[tauri::command]
fn get_file_diff(repo_path: String, base_branch: String, file_path: String) -> Result<git::FileDiff, String> {
    git::get_file_diff(&repo_path, &base_branch, &file_path)
}

#[tauri::command]
fn detect_default_branch(repo_path: String) -> Result<String, String> {
    git::detect_default_branch(&repo_path)
}

#[tauri::command]
fn list_monospace_fonts() -> Result<Vec<String>, String> {
    use font_kit::source::SystemSource;
    use font_kit::properties::Properties;
    use font_kit::family_name::FamilyName;
    use std::sync::OnceLock;

    static CACHE: OnceLock<Vec<String>> = OnceLock::new();

    Ok(CACHE.get_or_init(|| {
        let source = SystemSource::new();
        let all_families = source.all_families().unwrap_or_default();

        let mut fonts: Vec<String> = all_families
            .into_iter()
            .filter(|name| !name.starts_with('.'))
            .filter(|name| {
                source
                    .select_best_match(&[FamilyName::Title(name.clone())], &Properties::new())
                    .ok()
                    .and_then(|handle| handle.load().ok())
                    .map(|font| font.is_monospace())
                    .unwrap_or(false)
            })
            .collect();
        fonts.sort();
        fonts.dedup();
        fonts
    }).clone())
}

#[tauri::command]
fn attach_session(session_id: String, on_data: Channel<tauri::ipc::Response>, db_state: State<DbState>, config_state: State<ConfigState>, state: State<PtyState>, notify: State<NotifyHandle>, app: tauri::AppHandle) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    // Capture discovery info before moving session fields
    let discovery_info = if session.backend != "tmux" {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let provider_key = session.provider.as_deref().unwrap_or(&cfg.default_provider);
        let provider_def = cfg.providers.get(provider_key)
            .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;

        let list_cmd = provider_def.list_sessions_command.clone();
        let pattern = provider_def.session_id_pattern.clone();

        // Don't resume if another active session already holds this provider_session_id
        let resume_id = session.provider_session_id.as_deref().and_then(|pid| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE provider_session_id = ?1 AND status = 'active' AND id != ?2",
                rusqlite::params![pid, &session_id],
                |r| r.get(0),
            ).unwrap_or(0);
            if count > 0 { None } else { Some(pid) }
        });
        let is_resume = resume_id.is_some() && provider_def.resume_flag.is_some();

        let cmd = if is_resume {
            config::restart_command_for_provider(provider_def, resume_id)
        } else {
            config::launch_command(provider_def, session.auto_approve)
        };
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let command = resolve_command(parts[0]);
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
        let project_path = projects.iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.path.as_str())
            .unwrap_or("/");
        let cwd = session.worktree_path.as_deref().unwrap_or(project_path).to_string();

        Some((list_cmd, pattern, is_resume, session.provider_session_id.clone(), cwd.clone(), command, args, cwd))
    } else {
        None
    };

    let pty_target = if session.backend == "tmux" {
        let tmux_name = session.tmux_name.ok_or("tmux session has no tmux_name")?;
        pty::PtyTarget::TmuxAttach { tmux_name }
    } else {
        let (_, _, _, _, _, ref command, ref args, ref cwd) = discovery_info.as_ref().unwrap();
        pty::PtyTarget::Direct { command: command.clone(), args: args.clone(), cwd: cwd.clone() }
    };

    state.0.attach(&session_id, pty_target, app.clone(), on_data)?;

    // Register with notification system
    {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
        let project_name = projects.iter()
            .find(|p| p.id == session.project_id)
            .map(|p| p.name.as_str())
            .unwrap_or("unknown");
        let display_name = if session.name.is_empty() { &session.branch } else { &session.name };
        let hook_enabled = session.provider.as_deref()
            .map(|pk| provider_has_hook(pk, &cfg))
            .unwrap_or(false);
        let mut ns = notify.0.lock().unwrap();
        ns.register_session(&session_id, display_name, project_name, hook_enabled);
    }

    // If session was exited, mark it active again (auto-restart on reopen)
    if session.status == "exited" {
        db::restore_session(&conn, &session_id).map_err(|e| e.to_string())?;
    }
    drop(conn);

    // Spawn background discovery thread for direct sessions
    if let Some((Some(list_cmd), Some(pattern), is_resume, previous_id, cwd, _, _, _)) = discovery_info {
        eprintln!("[DEBUG-disc] spawning discovery thread for session={}, list_cmd='{}', cwd='{}'", &session_id, &list_cmd, &cwd);
        let sid = session_id.clone();
        let db_path = app.path().app_data_dir().expect("app data dir").join("planeai.db");
        std::thread::spawn(move || {
            discover_provider_session_id(&sid, &list_cmd, &pattern, &cwd, previous_id.as_deref(), is_resume, &db_path, &app);
        });
    } else {
        eprintln!("[DEBUG-disc] skipping discovery: discovery_info={:?}", discovery_info.as_ref().map(|(a, b, _, _, _, _, _, _)| (a.is_some(), b.is_some())));
    }

    Ok(())
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
fn pause_pty(session_id: String, state: State<PtyState>) -> Result<(), String> {
    state.0.pause(&session_id)
}

#[tauri::command]
fn resume_pty(session_id: String, state: State<PtyState>) -> Result<(), String> {
    state.0.resume(&session_id)
}

#[tauri::command]
fn check_session_alive(tmux_name: String) -> bool {
    #[cfg(not(windows))]
    { tmux::has_session(&tmux_name) }
    #[cfg(windows)]
    { false }
}

#[tauri::command]
fn is_notify_hook_installed(config_state: State<ConfigState>) -> bool {
    let cfg = config_state.0.lock().unwrap();
    let supported: Vec<_> = cfg.providers.values()
        .filter(|p| p.command.contains("kiro") || p.command.contains("claude"))
        .collect();
    if supported.is_empty() {
        return true; // no supported providers, nothing to install
    }
    supported.iter().all(|p| {
        if p.command.contains("kiro") { is_kiro_hook_installed() }
        else { is_claude_hook_installed() }
    })
}

#[tauri::command]
fn install_notify_hook(config_state: State<ConfigState>) -> Result<(), String> {
    let cfg = config_state.0.lock().unwrap();
    let home = config::home_dir();

    // Install for Kiro if configured
    if cfg.providers.values().any(|p| p.command.contains("kiro")) {
        install_kiro_hook(&home)?;
    }

    // Install for Claude Code if configured
    if cfg.providers.values().any(|p| p.command.contains("claude")) {
        install_claude_hook(&home)?;
    }

    Ok(())
}

fn install_kiro_hook(home: &str) -> Result<(), String> {
    let hooks_dir = format!("{home}/.kiro/hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.sh"),
        include_str!("../resources/planeai-stop-notify.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        format!("{hooks_dir}/planeai-stop-notify.ps1"),
        include_str!("../resources/planeai-stop-notify.ps1"),
    );

    std::fs::write(&script_path, script_content).map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    // Patch the default agent config
    let agents_dir = format!("{home}/.kiro/agents");
    std::fs::create_dir_all(&agents_dir).map_err(|e| format!("failed to create agents dir: {e}"))?;
    let config_path = format!("{agents_dir}/default.json");

    let mut config: serde_json::Value = if let Ok(content) = std::fs::read_to_string(&config_path) {
        serde_json::from_str(&content).map_err(|e| format!("failed to parse default.json: {e}"))?
    } else {
        serde_json::json!({ "name": "default", "tools": ["*"] })
    };

    let hooks = config.as_object_mut().unwrap()
        .entry("hooks").or_insert_with(|| serde_json::json!({}));
    let stop_hooks = hooks.as_object_mut().unwrap()
        .entry("stop").or_insert_with(|| serde_json::json!([]));
    let stop_arr = stop_hooks.as_array_mut().unwrap();

    let already = stop_arr.iter().any(|h| {
        h.get("command").and_then(|c| c.as_str()).map_or(false, |c| c.contains("planeai-stop-notify"))
    });
    if !already {
        #[cfg(not(windows))]
        let hook_command = format!("{hooks_dir}/planeai-stop-notify.sh");
        #[cfg(windows)]
        let hook_command = format!("powershell -NoProfile -File \"{hooks_dir}/planeai-stop-notify.ps1\"");
        stop_arr.push(serde_json::json!({ "command": hook_command }));
    }

    let output = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, output).map_err(|e| format!("failed to write default.json: {e}"))?;
    Ok(())
}

fn install_claude_hook(home: &str) -> Result<(), String> {
    let claude_dir = std::path::PathBuf::from(format!("{home}/.claude"));
    let hooks_dir = claude_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;

    #[cfg(not(windows))]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.sh"),
        include_str!("../resources/planeai-stop-notify-claude.sh"),
    );
    #[cfg(windows)]
    let (script_path, script_content) = (
        hooks_dir.join("planeai-stop-notify-claude.ps1"),
        include_str!("../resources/planeai-stop-notify-claude.ps1"),
    );

    std::fs::write(&script_path, script_content).map_err(|e| format!("failed to write hook script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to chmod hook: {e}"))?;
    }

    let script_command = script_path.to_string_lossy().to_string();
    notify::install_claude_hook_at(&claude_dir, &script_command)
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
fn spawn_tab(session_id: String, tab_index: u32, on_data: Channel<tauri::ipc::Response>, db_state: State<DbState>, state: State<PtyState>, app: tauri::AppHandle) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
    let project_path = projects.iter()
        .find(|p| p.id == session.project_id)
        .map(|p| p.path.as_str())
        .unwrap_or("/");
    let cwd = session.worktree_path.as_deref().unwrap_or(project_path).to_string();

    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) { "cmd.exe".to_string() } else { "/bin/zsh".to_string() }
    });

    let pty_key = format!("{}:{}", session_id, tab_index);
    let target = pty::PtyTarget::Direct {
        command: shell,
        args: vec!["-l".to_string()],
        cwd,
    };
    state.0.attach(&pty_key, target, app, on_data)?;

    let new_count = session.tab_count + 1;
    db::update_tab_count(&conn, &session_id, new_count).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_tab(session_id: String, tab_index: u32, db_state: State<DbState>, state: State<PtyState>) -> Result<(), String> {
    let pty_key = format!("{}:{}", session_id, tab_index);
    state.0.detach(&pty_key);

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    let new_count = (session.tab_count - 1).max(1);
    db::update_tab_count(&conn, &session_id, new_count).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn check_tmux_available() -> bool {
    config::tmux_available()
}

#[tauri::command]
fn restart_session(session_id: String, db_state: State<DbState>, config_state: State<ConfigState>) -> Result<db::Session, String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;

    if session.status != "exited" {
        return Err("can only restart exited sessions".to_string());
    }

    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let provider_key = session.provider.as_deref().unwrap_or(&cfg.default_provider);
    let provider_def = cfg.providers.get(provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let has_resume = session.provider_session_id.is_some() && provider_def.resume_flag.is_some();
    let cmd = if has_resume {
        config::restart_command_for_provider(provider_def, session.provider_session_id.as_deref())
    } else {
        config::launch_command(provider_def, session.auto_approve)
    };
    drop(cfg);

    if session.backend == "tmux" {
        #[cfg(not(windows))]
        {
            let tmux_name = session.tmux_name.as_deref().ok_or("tmux session has no tmux_name")?;
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
            let project_path = projects.iter()
                .find(|p| p.id == session.project_id)
                .map(|p| p.path.as_str())
                .unwrap_or("/");
            let cwd = session.worktree_path.as_deref().unwrap_or(project_path);
            tmux::create_session_with_cmd(tmux_name, cwd, &cmd, &session_id)?;
        }
        #[cfg(windows)]
        return Err("tmux backend not available on Windows".to_string());
    }

    db::restore_session(&conn, &session_id).map_err(|e| e.to_string())?;
    let updated = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found after restore")?;

    // Fire on_restart lifecycle hook
    if updated.task_key.is_some() {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        if let Some(cwd) = session_cwd(&conn, &updated) {
            fire_task_hook(&cfg, &updated, "on_restart", &cwd);
        }
    }

    Ok(updated)
}

#[tauri::command]
fn archive_session(id: String, db_state: State<DbState>, pty_state: State<PtyState>, config_state: State<ConfigState>) -> Result<(), String> {
    pty_state.0.detach(&id);
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;

    // Fire on_complete before archiving
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        if session.task_key.is_some() {
            let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
            if let Some(cwd) = session_cwd(&conn, &session) {
                fire_task_hook(&cfg, &session, "on_complete", &cwd);
            }
        }
    }

    db::archive_session(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn destroy_session(id: String, db_state: State<DbState>, pty_state: State<PtyState>, config_state: State<ConfigState>) -> Result<(), String> {
    // Detach PTY
    pty_state.0.detach(&id);

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    if let Some(session) = db::get_session(&conn, &id).map_err(|e| e.to_string())? {
        // Fire on_complete before destroying
        if session.task_key.is_some() {
            let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
            if let Some(cwd) = session_cwd(&conn, &session) {
                fire_task_hook(&cfg, &session, "on_complete", &cwd);
            }
        }

        // Only kill tmux if this is a tmux-backed session
        if session.backend == "tmux" {
            #[cfg(not(windows))]
            if let Some(ref tn) = session.tmux_name {
                let _ = tmux::kill_session(tn);
            }
        }
        // Remove worktree if applicable
        if let Some(ref wt_path) = session.worktree_path {
            let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
            if let Some(project) = projects.iter().find(|p| p.id == session.project_id) {
                let _ = git::worktree_remove(&project.path, wt_path);
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
    task_key: Option<String>,
    task_prompt: Option<String>,
) -> Result<db::Session, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let provider_key = provider.unwrap_or_else(|| cfg.default_provider.clone());
    let provider_def = cfg.providers.get(&provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let mut cmd = config::launch_command(provider_def, auto_approve);

    // Append prompt_command if task prompt is provided
    if let (Some(prompt), Some(prompt_cmd_template)) = (&task_prompt, &provider_def.prompt_command) {
        let mut vars = std::collections::HashMap::new();
        vars.insert("prompt", prompt.as_str());
        let rendered = template::render(prompt_cmd_template, &vars);
        let escaped = shell_escape(&rendered);
        cmd = format!("{cmd} {escaped}");
    }

    let hook_enabled = provider_has_hook(&provider_key, &cfg);
    let backend = config::resolve_backend(&cfg).to_string();
    drop(cfg);

    let conn = state.0.lock().map_err(|e| e.to_string())?;

    // Detect base branch before any checkout/worktree operation
    let effective_base_branch = base_branch.clone().or_else(|| {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo_path)
            .output()
            .ok()?;
        if output.status.success() {
            let b = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !b.is_empty() && b != "HEAD" { Some(b) } else { None }
        } else { None }
    });

    let (working_dir, worktree_path) = if use_worktree {
        let base = base_branch.as_deref().unwrap_or("main");
        let session_id = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
        let sanitized_project = project_name.replace(' ', "-").to_lowercase();
        let home = config::home_dir();
        let wt_path = format!("{home}/.planeai/worktrees/{sanitized_project}/{session_id}");
        std::fs::create_dir_all(std::path::Path::new(&wt_path).parent().unwrap())
            .map_err(|e| format!("failed to create worktree dir: {e}"))?;
        git::worktree_add(&repo_path, &wt_path, &branch, base)?;
        (wt_path.clone(), Some(wt_path))
    } else {
        git::checkout_branch(&repo_path, &branch, is_new_branch, base_branch.as_deref())?;
        (repo_path.clone(), None)
    };

    let session_id = uuid::Uuid::new_v4().to_string();

    let tmux_name: Option<String> = if backend == "tmux" {
        #[cfg(not(windows))]
        {
            let tn = tmux::session_name(&project_name);
            tmux::create_session_with_cmd(&tn, &working_dir, &cmd, &session_id)?;
            Some(tn)
        }
        #[cfg(windows)]
        return Err("tmux backend not available on Windows".to_string());
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
    let session = db::create_session_with_id(&conn, &session_id, &project_id, &name, tmux_name.as_deref(), &branch, worktree_path.as_deref(), Some(&provider_key), &backend, auto_approve, task_key.as_deref(), effective_base_branch.as_deref())
        .map_err(|e| e.to_string())?;

    // Fire on_start lifecycle hook
    if session.task_key.is_some() {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        fire_task_hook(&cfg, &session, "on_start", &repo_path);
    }

    Ok(session)
}

/// Fire a task manager lifecycle hook (on_start, on_notify, on_restart, on_complete).
/// If the session has a task_key and the configured task manager has the hook, executes move_task.
fn fire_task_hook(cfg: &config::Config, session: &db::Session, hook_name: &str, cwd: &str) {
    let task_key = match &session.task_key {
        Some(k) => k,
        None => return,
    };
    let tm = match resolve_task_manager(cfg) {
        Ok(tm) => tm,
        Err(_) => return,
    };
    let hook = match hook_name {
        "on_start" => tm.on_start.as_ref(),
        "on_notify" => tm.on_notify.as_ref(),
        "on_restart" => tm.on_restart.as_ref(),
        "on_complete" => tm.on_complete.as_ref(),
        _ => None,
    };
    if let Some(h) = hook {
        let _ = task_manager::move_task(tm, task_key, &h.move_to, std::path::Path::new(cwd));
    }
}

/// Resolve the working directory for a session's project.
fn session_cwd(conn: &rusqlite::Connection, session: &db::Session) -> Option<String> {
    if let Some(ref wt) = session.worktree_path {
        return Some(wt.clone());
    }
    db::list_projects(conn).ok()
        .and_then(|ps| ps.into_iter().find(|p| p.id == session.project_id))
        .map(|p| p.path)
}

#[tauri::command]
fn get_task_details(
    config_state: State<ConfigState>,
    key: String,
    repo_path: String,
) -> Result<task_manager::TaskItem, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let tm = resolve_task_manager(&cfg)?;
    task_manager::get_task(tm, &key, std::path::Path::new(&repo_path))
}

#[tauri::command]
fn fire_task_notify_hook(
    session_id: String,
    db_state: State<DbState>,
    config_state: State<ConfigState>,
) -> Result<(), String> {
    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    let session = db::get_session(&conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    if session.task_key.is_some() {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        if let Some(cwd) = session_cwd(&conn, &session) {
            fire_task_hook(&cfg, &session, "on_notify", &cwd);
        }
    }
    Ok(())
}

#[tauri::command]
fn list_task_items(
    config_state: State<ConfigState>,
    repo_path: String,
) -> Result<Vec<task_manager::TaskItem>, String> {
    let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
    let tm = resolve_task_manager(&cfg)?;
    task_manager::list_tasks(tm, std::path::Path::new(&repo_path))
}

fn resolve_task_manager(cfg: &config::Config) -> Result<&config::TaskManager, String> {
    let key = cfg.default_task_manager.as_deref()
        .or_else(|| cfg.task_managers.keys().next().map(|s| s.as_str()))
        .ok_or("No task manager configured")?;
    cfg.task_managers.get(key)
        .ok_or_else(|| format!("Task manager '{}' not found in config", key))
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
            #[cfg(not(windows))]
            let _ = db::reconcile_sessions(&conn, |name| tmux::has_session(name));
            #[cfg(windows)]
            let _ = db::reconcile_sessions(&conn, |_| false);

            // Config: migrate from DB if needed, then load
            let config_dir = config::config_dir(&app.package_info().name);
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
            #[cfg(unix)]
            pty_mgr.set_socket_path(notify::socket_path(&app_dir).to_string_lossy().into_owned());
            #[cfg(windows)]
            pty_mgr.set_socket_path(notify::PIPE_NAME.to_string());
            app.manage(PtyState(pty_mgr));

            // Warm font cache in background so preferences page opens instantly
            std::thread::spawn(|| { let _ = list_monospace_fonts(); });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            list_projects,
            list_archived_projects,
            archive_project,
            restore_project,
            delete_project,
            create_session,
            list_sessions,
            delete_session,
            rename_session,
            list_archived_sessions,
            restore_session,
            validate_git_repo,
            list_branches,
            get_changed_files,
            get_file_diff,
            detect_default_branch,
            list_monospace_fonts,
            get_config,
            update_config,
            launch_session,
            attach_session,
            write_to_pty,
            resize_pty,
            pause_pty,
            resume_pty,
            check_session_alive,
            is_notify_hook_installed,
            install_notify_hook,
            acknowledge_session,
            mark_exited,
            spawn_tab,
            close_tab,
            check_tmux_available,
            restart_session,
            archive_session,
            destroy_session,
            get_task_details,
            list_task_items,
            fire_task_notify_hook,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
