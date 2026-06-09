use tauri::{Emitter, State};

use crate::config;
use crate::db;
use crate::git;
use crate::state::{ConfigState, DbState, NotifyHandle};
use crate::template;
#[cfg(not(windows))]
use crate::tmux;
use crate::util::{resolve_command, sanitize_project_name, shell_escape};

use super::helpers::{fire_task_hook, provider_has_hook};

/// Background discovery of provider session ID with retry-backoff.
#[allow(clippy::too_many_arguments)]
pub(crate) fn discover_provider_session_id(
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
        eprintln!("[DEBUG-disc] attempt after {delay}s: running '{list_cmd}' in cwd '{cwd}'");
        let output = match planeai_core::command::run_command(list_cmd, std::path::Path::new(cwd)) {
            Ok(stdout) => stdout,
            Err(e) => {
                eprintln!("[DEBUG-disc] command failed: {e}");
                continue;
            }
        };
        eprintln!("[DEBUG-disc] success, stdout_len={}", output.len(),);
        let discovered = config::parse_provider_session_id(&output, pattern);
        eprintln!(
            "[DEBUG-disc] parsed session_id={:?}, previous={:?}, is_resume={}",
            discovered, previous_id, is_resume
        );
        if config::should_accept_provider_session_id(discovered.as_deref(), previous_id, is_resume)
        {
            eprintln!(
                "[DEBUG-disc] accepted! storing provider_session_id={:?}",
                discovered
            );
            if let Ok(conn) = rusqlite::Connection::open(db_path) {
                let _ =
                    db::set_provider_session_id(&conn, session_id, discovered.as_ref().unwrap());
            }
            return;
        } else {
            eprintln!("[DEBUG-disc] rejected (stale or no match)");
            last_discovered = discovered;
        }
    }
    if is_resume {
        if let Some(new_id) = last_discovered {
            eprintln!(
                "[DEBUG-disc] resume failed, accepting new session id={}",
                new_id
            );
            if let Ok(conn) = rusqlite::Connection::open(db_path) {
                let _ = db::set_provider_session_id(&conn, session_id, &new_id);
            }
            return;
        }
    }
    let _ = app.emit(
        "provider-session-id-failed",
        serde_json::json!({
            "session_id": session_id,
            "reason": "Could not discover provider session ID after retries"
        }),
    );
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn launch_session(
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
    let provider_def = cfg
        .providers
        .get(&provider_key)
        .ok_or_else(|| format!("Unknown provider: {provider_key}"))?;
    let mut cmd = config::launch_command(provider_def, auto_approve);

    if let (Some(prompt), Some(prompt_cmd_template)) = (&task_prompt, &provider_def.prompt_command)
    {
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
            if !b.is_empty() && b != "HEAD" {
                Some(b)
            } else {
                None
            }
        } else {
            None
        }
    });

    let (working_dir, worktree_path) = if use_worktree {
        let base = base_branch.as_deref().unwrap_or("main");
        let session_id = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
        let sanitized_project = sanitize_project_name(&project_name);
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

    {
        let mut ns = notify.0.lock().unwrap();
        let display_name = if name.is_empty() { &branch } else { &name };
        ns.register_session(&session_id, display_name, &project_name, hook_enabled);
    }

    let session = db::create_session_with_id(
        &conn,
        &session_id,
        &project_id,
        &name,
        tmux_name.as_deref(),
        &branch,
        worktree_path.as_deref(),
        Some(&provider_key),
        &backend,
        auto_approve,
        task_key.as_deref(),
        effective_base_branch.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    if session.task_key.is_some() {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        fire_task_hook(&cfg, &session, "on_start", &repo_path);
    }

    Ok(session)
}
