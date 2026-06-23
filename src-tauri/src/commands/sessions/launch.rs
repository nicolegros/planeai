use tauri::{AppHandle, Emitter, Manager, State};

use planeai_tasks::model::DEFAULT_BASE_BRANCH;

use crate::config;
use crate::db;
use crate::git;
use crate::state::{ConfigState, DaemonState, DbState, NotifyHandle};
#[cfg(not(windows))]
use crate::tmux;
use crate::util::sanitize_project_name;

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
pub async fn launch_session(
    app: AppHandle,
    state: State<'_, DbState>,
    notify: State<'_, NotifyHandle>,
    config_state: State<'_, ConfigState>,
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
    tracing::info!(task_prompt = ?task_prompt, auto_approve, provider = ?provider, task_key = ?task_key, "launch_session called");
    // Phase 1: gather params from config (holding config lock briefly)
    let (cmd, provider_key, hook_enabled, backend, scrollback_bytes, extra_path_dirs) = {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let pk = provider.unwrap_or_else(|| cfg.default_provider.clone());
        let provider_def = cfg
            .providers
            .get(&pk)
            .ok_or_else(|| format!("Unknown provider: {pk}"))?;

        let core_provider = planeai_core::session_launch::ProviderConfig {
            command: provider_def.command.clone(),
            yolo_flag: provider_def.yolo_flag.clone(),
            prompt_command: provider_def.prompt_command.clone(),
            autonomous_prompt_template: provider_def.autonomous_prompt_template.clone(),
        };
        let launch_cmd = planeai_core::session_launch::build_provider_launch_command(
            &core_provider,
            auto_approve,
            task_prompt.as_deref(),
            false, // manual launches are not autonomous
        );
        let c = launch_cmd.command;
        tracing::info!(command = %c, prompt_injected = launch_cmd.prompt_was_injected, approve_applied = launch_cmd.auto_approve_was_applied, "launch command built");

        let he = provider_has_hook(&pk, &cfg);
        let be = config::resolve_backend(&cfg).to_string();
        let sb = 1_048_576;
        let epd: Vec<String> = cfg
            .extra_path_dirs
            .iter()
            .map(|d| crate::util::expand_tilde(d))
            .collect();
        (c, pk, he, be, sb, epd)
    };

    // Phase 2: sync work — detect base branch, git worktree/checkout
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
        let base = base_branch.as_deref().unwrap_or(DEFAULT_BASE_BRANCH);
        let short_id = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
        let sanitized_project = sanitize_project_name(&project_name);
        let home = config::home_dir();
        let wt_path = format!("{home}/.planeai/worktrees/{sanitized_project}/{short_id}");
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
            tmux::create_session_with_cmd_and_path(&tn, &working_dir, &cmd, &session_id, &extra_path_dirs)?;
            Some(tn)
        }
        #[cfg(windows)]
        return Err("tmux backend not available on Windows".to_string());
    } else {
        None
    };

    // Phase 3: async daemon work — no locks held
    if backend == "daemon" {
        let daemon_state = app.state::<DaemonState>();
        let socket_path = planeai_ipc::daemon_socket_path();
        let sidecar_path = crate::paths::resolve_daemon_binary(&app);

        crate::daemon_client::ensure_daemon_running(&sidecar_path, &socket_path, scrollback_bytes)
            .await?;

        let launch_req = planeai_core::session_launch::CreateSessionRequest {
            session_id: session_id.clone(),
            project_cwd: std::path::PathBuf::from(&working_dir),
            session_target: planeai_core::session_launch::SessionTarget::Daemon,
            agent_command: cmd.clone(),
            env: std::collections::HashMap::new(),
            extra_path_dirs: extra_path_dirs.clone(),
            cols: 80,
            rows: 24,
            durable_logs: std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok(),
        };
        let launch_result = planeai_core::session_launch::prepare_session(&launch_req)
            .map_err(|e| e.to_string())?;

        tracing::info!(
            caller = "tauri",
            shared_launch_service = true,
            target = "daemon",
            cwd = %launch_result.cwd.display(),
            command_label = %launch_result.command_label,
            durable_logs = launch_req.durable_logs,
            extra_path_dirs_count = launch_req.extra_path_dirs.len(),
            "session created via shared launch service"
        );

        let mut ds = daemon_state.0.lock().await;
        let client = match ds.as_mut() {
            Some(c) => c,
            None => {
                *ds = Some(
                    crate::daemon_client::DaemonClient::connect(&socket_path)
                        .await
                        .map_err(|e| format!("daemon connect failed: {e}"))?,
                );
                ds.as_mut().unwrap()
            }
        };
        client
            .spawn_session(
                &launch_result.session_id,
                &launch_result.program,
                &launch_result.args,
                &working_dir,
                Some(&launch_result.env),
            )
            .await?;
    }

    // Phase 4: DB write and notify (re-acquire lock)
    let conn = state.0.lock().map_err(|e| e.to_string())?;

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
        fire_task_hook(&cfg, &session, "on_start", &repo_path, &conn);
    }

    Ok(session)
}

#[cfg(test)]
mod tests {
    use planeai_core::command::shell_args;

    #[test]
    fn shell_args_preserves_quoted_prompt() {
        let cmd = "kiro-cli chat --trust-all-tools 'Implement PLA-89: fix daemon task launch'";
        let (program, args) = shell_args(cmd);

        if cfg!(windows) {
            assert_eq!(program, "cmd");
            assert_eq!(args, vec!["/C", cmd]);
        } else {
            assert_eq!(program, "/bin/sh");
            assert_eq!(args, vec!["-c", cmd]);
        }
    }

    #[test]
    fn shell_args_handles_simple_command() {
        let cmd = "kiro-cli chat";
        let (program, args) = shell_args(cmd);

        if cfg!(windows) {
            assert_eq!(program, "cmd");
            assert_eq!(args, vec!["/C", cmd]);
        } else {
            assert_eq!(program, "/bin/sh");
            assert_eq!(args, vec!["-c", cmd]);
        }
    }
}
