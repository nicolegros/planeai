use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use planeai_core::command::augmented_path;
use planeai_tasks::model::DEFAULT_BASE_BRANCH;

use crate::config;
use crate::db;
use crate::git;
use crate::state::{ConfigState, DaemonState, DbState, NotifyHandle, ProjectOperationState};
#[cfg(not(windows))]
use crate::tmux;
use crate::util::sanitize_project_name;

use super::helpers::{fire_task_hook, provider_has_hook};

/// Result of launching a session, with an optional warning for the frontend to display.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchResult {
    pub session: db::Session,
    pub warning: Option<String>,
}

/// Check if a git error indicates a worktree conflict (branch already checked out).
fn is_worktree_conflict(e: &str) -> bool {
    e.contains("already checked out") || e.contains("already used by worktree")
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn launch_session(
    app: AppHandle,
    state: State<'_, DbState>,
    notify: State<'_, NotifyHandle>,
    config_state: State<'_, ConfigState>,
    operations: State<'_, ProjectOperationState>,
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
) -> Result<LaunchResult, String> {
    // Tauri accepts these client-provided fields for compatibility; the authoritative values
    // are loaded below after acquiring the project operation lock.
    let _ = (&project_name, &repo_path);
    let operation_lock = operations.lock_for(&project_id);
    let _operation_guard = operation_lock.lock_owned().await;
    let (project_name, repo_path) = crate::commands::blocking({
        let conn = state.0.clone();
        let project_id = project_id.clone();
        move || {
            let conn = conn.lock().map_err(|e| e.to_string())?;
            let project = db::get_project(&conn, &project_id)
                .map_err(|e| e.to_string())?
                .filter(|project| project.status == "active")
                .ok_or_else(|| "Project not found or archived.".to_string())?;
            Ok((project.name, project.path))
        }
    })
    .await?;
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
        };
        let launch_cmd = planeai_core::session_launch::build_provider_launch_command(
            &core_provider,
            auto_approve,
            task_prompt.as_deref(),
            false, // manual launches are not autonomous
            None,  // autonomous_prompt_template not used for manual launches
        );
        let c = launch_cmd.command;
        tracing::info!(command = %c, prompt_injected = launch_cmd.prompt_was_injected, approve_applied = launch_cmd.auto_approve_was_applied, "launch command built");

        let he = provider_has_hook(&pk, &cfg);
        let be = config::resolve_backend(&cfg).to_string();
        let sb = 1_048_576;
        let epd = cfg.resolved_extra_path_dirs();
        (c, pk, he, be, sb, epd)
    };

    // Phase 2: async work — detect base branch, git worktree/checkout
    let effective_base_branch = {
        let repo_path = repo_path.clone();
        let base_branch = base_branch.clone();
        crate::commands::blocking(move || {
            Ok(base_branch.or_else(|| {
                let mut cmd = std::process::Command::new(crate::command::resolve("git"));
                cmd.args(["rev-parse", "--abbrev-ref", "HEAD"])
                    .current_dir(&repo_path);
                cmd.env("PATH", augmented_path(&[]));
                planeai_core::command::no_window(&mut cmd);
                let output = cmd.output().ok()?;
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
            }))
        })
        .await?
    };

    let mut warning: Option<String> = None;

    let (working_dir, worktree_path, created_worktree, created_branch) = if use_worktree {
        let base = base_branch.as_deref().unwrap_or(DEFAULT_BASE_BRANCH);
        let short_id = uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
        let sanitized_project = sanitize_project_name(&project_name);
        let home = config::home_dir();
        let wt_path = format!("{home}/.planeai/worktrees/{sanitized_project}/{short_id}");
        std::fs::create_dir_all(std::path::Path::new(&wt_path).parent().unwrap())
            .map_err(|e| format!("failed to create worktree dir: {e}"))?;
        match git::worktree_add(&repo_path, &wt_path, &branch, base) {
            Ok(()) => (wt_path.clone(), Some(wt_path), true, true),
            Err(e) if is_worktree_conflict(&e) => {
                // Branch already checked out in an existing worktree — reuse it
                let existing_wt =
                    git::find_worktree_for_branch(&repo_path, &branch).ok_or_else(|| e.clone())?;
                tracing::info!(
                    branch = %branch,
                    worktree = %existing_wt,
                    "branch already in worktree, reusing"
                );
                warning = Some(format!(
                    "Branch '{branch}' is already in a worktree — session will run there"
                ));
                (existing_wt.clone(), Some(existing_wt), false, false)
            }
            Err(e) => return Err(e),
        }
    } else {
        git::checkout_branch(&repo_path, &branch, is_new_branch, base_branch.as_deref())?;
        (repo_path.clone(), None, false, is_new_branch)
    };

    let session_id = uuid::Uuid::new_v4().to_string();

    let tmux_name: Option<String> = if backend == "tmux" {
        #[cfg(not(windows))]
        {
            let tn = tmux::session_name(&project_name);
            tmux::create_session_with_cmd_and_path(
                &tn,
                &working_dir,
                &cmd,
                &session_id,
                &extra_path_dirs,
            )?;
            Some(tn)
        }
        #[cfg(windows)]
        return Err("tmux backend not available on Windows".to_string());
    } else {
        None
    };

    // Phase 3: async daemon work — no locks held
    if backend == "daemon" {
        let spawn_result = spawn_in_daemon(
            &app,
            &session_id,
            &working_dir,
            &cmd,
            &extra_path_dirs,
            scrollback_bytes,
        )
        .await;

        if let Err(e) = spawn_result {
            {
                let rp = repo_path.clone();
                let br = branch.clone();
                let wtp = if created_worktree {
                    worktree_path.clone()
                } else {
                    None
                };
                let _ = crate::commands::blocking(move || {
                    rollback_branch_creation(&rp, &br, wtp.as_deref(), created_branch);
                    Ok(())
                })
                .await;
            }
            // Clear the stale daemon connection so next attempt reconnects automatically
            if e.contains("Broken pipe")
                || e.contains("Connection refused")
                || e.contains("No such file")
            {
                let daemon_state = app.state::<DaemonState>();
                let mut ds = daemon_state.0.lock().await;
                *ds = None;
                return Err("Session daemon is not responding — it may have crashed. Try again (the daemon will restart automatically).".to_string());
            }
            return Err(format!("Failed to launch session: {e}"));
        }
    }

    // Phase 4: DB write and notify (re-acquire lock)
    let conn = state.0.lock().map_err(|e| {
        let rp = repo_path.clone();
        let br = branch.clone();
        let wtp = if created_worktree {
            worktree_path.clone()
        } else {
            None
        };
        tokio::task::spawn_blocking(move || {
            rollback_branch_creation(&rp, &br, wtp.as_deref(), created_branch);
        });
        e.to_string()
    })?;

    {
        let mut ns = notify.0.lock().unwrap();
        let display_name = if name.is_empty() { &branch } else { &name };
        ns.register_session(&session_id, display_name, &project_name, hook_enabled);
    }

    let session = db::create_session_with_id_and_worktree_ownership(
        &conn,
        &session_id,
        &project_id,
        &name,
        tmux_name.as_deref(),
        &branch,
        worktree_path.as_deref(),
        created_worktree,
        Some(&provider_key),
        &backend,
        auto_approve,
        task_key.as_deref(),
        effective_base_branch.as_deref(),
        None,
    )
    .map_err(|e| {
        let rp = repo_path.clone();
        let br = branch.clone();
        let wtp = if created_worktree {
            worktree_path.clone()
        } else {
            None
        };
        tokio::task::spawn_blocking(move || {
            rollback_branch_creation(&rp, &br, wtp.as_deref(), created_branch);
        });
        e.to_string()
    })?;

    if session.task_key.is_some() {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        fire_task_hook(&cfg, &session, "on_start", &repo_path, &conn);
    }

    // If we reused an existing worktree, check if another active session is already there
    if let (Some(ref warn_msg), Some(ref wt)) = (&warning, &session.worktree_path) {
        let existing_session_name: Option<String> = conn
            .query_row(
                "SELECT name FROM sessions WHERE worktree_path = ?1 AND status = 'active' AND id != ?2 LIMIT 1",
                rusqlite::params![wt, &session.id],
                |row| row.get(0),
            )
            .ok();
        if let Some(other_name) = existing_session_name {
            warning = Some(format!(
                "{warn_msg} (session '{other_name}' is also running there)"
            ));
        }
    }

    Ok(LaunchResult { session, warning })
}

/// Attempt to spawn a session in the daemon. Returns an error string on failure.
async fn spawn_in_daemon(
    app: &AppHandle,
    session_id: &str,
    working_dir: &str,
    cmd: &str,
    extra_path_dirs: &[String],
    scrollback_bytes: usize,
) -> Result<(), String> {
    let daemon_state = app.state::<DaemonState>();
    let socket_path = planeai_ipc::daemon_socket_path();
    let sidecar_path = crate::paths::resolve_daemon_binary(app);

    crate::daemon_client::ensure_daemon_running(&sidecar_path, &socket_path, scrollback_bytes)
        .await?;

    let launch_req = planeai_core::session_launch::CreateSessionRequest {
        session_id: session_id.to_string(),
        project_cwd: std::path::PathBuf::from(working_dir),
        session_target: planeai_core::session_launch::SessionTarget::Daemon,
        agent_command: cmd.to_string(),
        env: std::collections::HashMap::new(),
        extra_path_dirs: extra_path_dirs.to_vec(),
        cols: 80,
        rows: 24,
        durable_logs: std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok(),
    };
    let launch_result =
        planeai_core::session_launch::prepare_session(&launch_req).map_err(|e| e.to_string())?;

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
    let result = client
        .spawn_session(
            &launch_result.session_id,
            &launch_result.program,
            &launch_result.args,
            working_dir,
            Some(&launch_result.env),
        )
        .await;

    // If the first attempt fails (e.g. broken pipe), the caller handles reconnection.
    // But if the error is "still running", the session was already spawned successfully
    // (the response was lost due to the broken pipe). Treat as success.
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.contains("still running") => {
            tracing::info!(session_id, "session already running, treating as success");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Roll back only branch/worktree resources created by this launch.
/// Best-effort — logs warnings but doesn't propagate errors.
fn rollback_branch_creation(
    repo_path: &str,
    branch: &str,
    worktree_path: Option<&str>,
    created_branch: bool,
) {
    if let Some(wt_path) = worktree_path {
        let errors = planeai_core::cleanup::cleanup_worktree(repo_path, wt_path, Some(branch));
        for e in &errors {
            tracing::warn!(error = %e, "rollback: worktree cleanup error");
        }
    } else if created_branch {
        let mut checkout_cmd = std::process::Command::new(crate::command::resolve("git"));
        checkout_cmd.args(["checkout", "-"]).current_dir(repo_path);
        checkout_cmd.env("PATH", augmented_path(&[]));
        planeai_core::command::no_window(&mut checkout_cmd);
        match checkout_cmd.output() {
            Ok(output) if !output.status.success() => {
                tracing::warn!(
                    stderr = %String::from_utf8_lossy(&output.stderr),
                    "rollback: git checkout failed, skipping branch delete"
                );
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, "rollback: failed to checkout previous branch");
                return;
            }
            _ => {}
        }
        let mut delete_cmd = std::process::Command::new(crate::command::resolve("git"));
        delete_cmd
            .args(["branch", "-D", branch])
            .current_dir(repo_path);
        delete_cmd.env("PATH", augmented_path(&[]));
        planeai_core::command::no_window(&mut delete_cmd);
        match delete_cmd.output() {
            Ok(output) if !output.status.success() => {
                tracing::warn!(
                    stderr = %String::from_utf8_lossy(&output.stderr),
                    "rollback: git branch -D failed"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "rollback: failed to delete branch");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use planeai_core::command::shell_args;

    use super::rollback_branch_creation;

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

    #[test]
    fn rollback_with_nonexistent_worktree_does_not_panic() {
        // rollback_branch_creation is best-effort — should never panic even
        // with paths/branches that don't exist.
        rollback_branch_creation(
            "/nonexistent/repo",
            "feat/nonexistent",
            Some("/nonexistent/wt"),
            true,
        );
    }

    #[test]
    fn rollback_without_worktree_and_not_new_branch_is_noop() {
        // When is_new_branch is false and no worktree, nothing should happen
        rollback_branch_creation("/nonexistent/repo", "main", None, false);
    }
}
