use serde::Serialize;
use tauri::{AppHandle, Manager};

use planeai_core::command::augmented_path;
use planeai_tasks::model::DEFAULT_BASE_BRANCH;

use crate::config;
use crate::db;
use crate::git;
use crate::plugin_providers;
use crate::session_ops::PLUGIN_BACKEND;
use crate::state::{ConfigState, DaemonState, DbState, NotifyHandle, ProjectOperationState};
#[cfg(not(windows))]
use crate::tmux;
use crate::util::sanitize_project_name;

use super::helpers::{fire_task_hook, register_notify_session};

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

/// Whether a daemon spawn failed because its connection is gone, so the next launch reconnects.
fn is_daemon_connection_error(e: &str) -> bool {
    e.contains("Broken pipe") || e.contains("Connection refused") || e.contains("No such file")
}

fn require_task_key(task_key: Option<String>) -> Result<String, String> {
    task_key
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| "A task is required to create a session.".to_string())
}

/// A session to launch for a task; the project's name and path are loaded from its row.
#[derive(Debug, Clone)]
pub(crate) struct LaunchRequest {
    pub project_id: String,
    pub branch: String,
    pub is_new_branch: bool,
    pub name: String,
    pub use_worktree: bool,
    pub base_branch: Option<String>,
    pub auto_approve: bool,
    pub provider: Option<String>,
    pub task_key: Option<String>,
    pub task_project_id: Option<String>,
    pub task_prompt: Option<String>,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn launch_session(
    app: AppHandle,
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
    task_project_id: Option<String>,
    task_prompt: Option<String>,
) -> Result<LaunchResult, String> {
    // Tauri accepts these client-provided fields for compatibility; `launch` loads the
    // authoritative values after acquiring the project operation lock.
    let _ = (&project_name, &repo_path);
    launch(
        &app,
        LaunchRequest {
            project_id,
            branch,
            is_new_branch,
            name,
            use_worktree,
            base_branch,
            auto_approve,
            provider,
            task_key,
            task_project_id,
            task_prompt,
        },
    )
    .await
}

/// Create the branch or worktree, spawn the agent on the configured backend and record the
/// session, undoing the git work when a later step fails.
pub(crate) async fn launch(
    app: &AppHandle,
    request: LaunchRequest,
) -> Result<LaunchResult, String> {
    let LaunchRequest {
        project_id,
        branch,
        is_new_branch,
        name,
        use_worktree,
        base_branch,
        auto_approve,
        provider,
        task_key,
        task_project_id,
        task_prompt,
    } = request;
    let state = app.state::<DbState>();
    let notify = app.state::<NotifyHandle>();
    let config_state = app.state::<ConfigState>();
    let operations = app.state::<ProjectOperationState>();
    let task_key = require_task_key(task_key)?;
    if let Some(prompt) = &task_prompt {
        planeai_core::session_launch::check_task_prompt(prompt)?;
    }
    let operation_lock = operations.lock_for(&project_id);
    let _operation_guard = operation_lock.lock_owned().await;
    let (project_name, repo_path) = crate::commands::blocking({
        let conn = state.0.clone();
        let project_id = project_id.clone();
        let task_project_id = task_project_id.clone();
        move || {
            let conn = conn.lock().map_err(|e| e.to_string())?;
            let project = db::get_project(&conn, &project_id)
                .map_err(|e| e.to_string())?
                .filter(|project| project.status == "active")
                .ok_or_else(|| "Project not found or archived.".to_string())?;
            if let Some(task_project_id) = &task_project_id {
                db::get_project(&conn, task_project_id)
                    .map_err(|e| e.to_string())?
                    .filter(|project| project.status == "active")
                    .ok_or_else(|| "Task project not found or archived.".to_string())?;
            }
            Ok((project.name, project.path))
        }
    })
    .await?;
    let workspace_project_id = task_project_id.as_deref().unwrap_or(&project_id);
    // The prompt is task content: log its size, never its text.
    let prompt_chars = task_prompt.as_deref().map(|prompt| prompt.chars().count());
    tracing::info!(prompt_chars = ?prompt_chars, auto_approve, provider = ?provider, task_key = ?task_key, "launch_session called");
    // Plugin providers have no command: their sidecar runs the agent (ADR-0014).
    let runtime_provider = match provider.as_deref() {
        Some(key) if plugin_providers::parse_provider_key(key).is_some() => {
            plugin_providers::resolve(&plugin_providers::AppRuntime::new(app), key).await?;
            Some(key.to_string())
        }
        _ => None,
    };
    // Phase 1: gather params from config (holding config lock briefly)
    let (cmd, provider_key, backend, scrollback_bytes, extra_path_dirs) = if let Some(key) =
        runtime_provider
    {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        (
            String::new(),
            key,
            PLUGIN_BACKEND.to_string(),
            0,
            cfg.resolved_extra_path_dirs(),
        )
    } else {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        let pk = provider.unwrap_or_else(|| cfg.default_provider.clone());
        let provider_def = cfg
            .providers
            .get(&pk)
            .ok_or_else(|| format!("Unknown provider: {pk}"))?;

        let c = provider_def.first_launch_command(auto_approve, task_prompt.as_deref());
        tracing::info!(provider_command = %provider_def.command, auto_approve, prompt_chars = ?prompt_chars, "launch command built");

        let be = config::resolve_backend(&cfg).to_string();
        let sb = 1_048_576;
        let epd = cfg.resolved_extra_path_dirs();
        (c, pk, be, sb, epd)
    };

    let wsl = {
        let wsl_config = config_state
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .wsl
            .clone();
        let plugin_owned = backend == PLUGIN_BACKEND;
        crate::commands::blocking(move || {
            Ok(wsl_config
                .filter(|_| !plugin_owned)
                .as_ref()
                .and_then(planeai_core::wsl::WslTarget::from_config))
        })
        .await?
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
        // Under WSL the worktree lives on the distro's own filesystem, where git is fast.
        let (home, git_ctx) = match &wsl {
            Some(target) => {
                let distro = target.distro.clone();
                let home = crate::commands::blocking(move || {
                    planeai_core::wsl::home_dir(&distro).map_err(|e| e.to_string())
                })
                .await?;
                (home, git::GitContext::wsl(&target.distro))
            }
            None => (config::home_dir(), git::GitContext::native()),
        };
        let wt_path = format!("{home}/.planeai/worktrees/{sanitized_project}/{short_id}");
        if wsl.is_none() {
            std::fs::create_dir_all(std::path::Path::new(&wt_path).parent().unwrap())
                .map_err(|e| format!("failed to create worktree dir: {e}"))?;
        }
        match git::worktree_add_in(&repo_path, &wt_path, &branch, base, &git_ctx) {
            Ok(()) => (wt_path.clone(), Some(wt_path), true, true),
            Err(e) if is_worktree_conflict(&e) => {
                // Branch already checked out in an existing worktree — reuse it
                let existing_wt = git::find_worktree_for_branch_in(&repo_path, &branch, &git_ctx)
                    .ok_or_else(|| e.clone())?;
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

    // Phase 3: async backend spawn — no locks held
    // Until the row exists, dropping this undoes the provider's start.
    let mut provider_launch = None;
    if backend == "daemon" || backend == planeai_rmux::BACKEND || backend == PLUGIN_BACKEND {
        let spawn_result = if backend == PLUGIN_BACKEND {
            let context = plugin_providers::SessionRuntimeContext {
                session_id: session_id.clone(),
                provider_key: provider_key.clone(),
                cwd: working_dir.clone(),
                auto_approve,
                extra_path_dirs: extra_path_dirs.clone(),
            };
            plugin_providers::launch(
                std::sync::Arc::new(plugin_providers::AppRuntime::new(app)),
                &context,
                task_prompt.as_deref(),
            )
            .await
            .map(|launch| provider_launch = Some(launch))
            .map_err(String::from)
        } else if backend == planeai_rmux::BACKEND {
            spawn_in_rmux(
                &session_id,
                workspace_project_id,
                Some(task_key.as_str()),
                &working_dir,
                &cmd,
                &extra_path_dirs,
                wsl.clone(),
            )
            .await
        } else {
            spawn_in_daemon(
                app,
                &session_id,
                &working_dir,
                &cmd,
                &extra_path_dirs,
                scrollback_bytes,
                wsl.clone(),
            )
            .await
        };

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
            // Clear the stale daemon connection so next attempt reconnects automatically.
            // The rmux backend needs no special case here: every rmux operation
            // already reconnects and retries once (`rmux_client::with_retry` and its siblings), so
            // a failure that reaches this point is a real one worth reporting
            // verbatim. Plugin providers fail for their own reasons, reported as is.
            if backend == "daemon" && is_daemon_connection_error(&e) {
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

    let pending_prompt = crate::session_ops::pending_prompt(&backend, task_prompt);
    let session = db::create_session_with_params(
        &conn,
        &planeai_core::services::CreateSessionParams {
            id: session_id,
            project_id,
            name,
            tmux_name,
            // Still needed by the rollback below.
            branch: branch.clone(),
            worktree_path: worktree_path.clone(),
            worktree_owned: Some(created_worktree),
            provider: Some(provider_key),
            backend,
            auto_approve,
            task_key: Some(task_key),
            task_project_id,
            base_branch: effective_base_branch,
            pending_prompt,
            ..Default::default()
        },
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
    if let Some(launch) = provider_launch {
        launch.commit();
    }

    {
        let cfg = config_state.0.lock().map_err(|e| e.to_string())?;
        register_notify_session(&mut notify.0.lock().unwrap(), &session, &project_name, &cfg);
        if session.task_key.is_some() {
            fire_task_hook(&cfg, &session, "on_start", &conn);
        }
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
    wsl: Option<planeai_core::wsl::WslTarget>,
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
        wsl,
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
            &launch_result.cwd.to_string_lossy(),
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

/// Spawn the agent in PlaneAI's private rmux daemon.
///
/// Reuses the shared launch service so the command, cwd, and augmented PATH are
/// built exactly as they are for every other backend.
async fn spawn_in_rmux(
    session_id: &str,
    workspace_project_id: &str,
    task_key: Option<&str>,
    working_dir: &str,
    cmd: &str,
    extra_path_dirs: &[String],
    wsl: Option<planeai_core::wsl::WslTarget>,
) -> Result<(), String> {
    let launch_req = planeai_core::session_launch::CreateSessionRequest {
        session_id: session_id.to_string(),
        project_cwd: std::path::PathBuf::from(working_dir),
        session_target: planeai_core::session_launch::SessionTarget::Daemon,
        agent_command: cmd.to_string(),
        env: std::collections::HashMap::new(),
        extra_path_dirs: extra_path_dirs.to_vec(),
        cols: crate::rmux_ops::DEFAULT_COLS,
        rows: crate::rmux_ops::DEFAULT_ROWS,
        durable_logs: std::env::var("PLANEAI_SESSION_LOG_DIR").is_ok(),
        wsl,
    };
    let launch_result =
        planeai_core::session_launch::prepare_session(&launch_req).map_err(|e| e.to_string())?;

    // Every agent on the same task shares one rmux workspace.
    let workspace =
        planeai_rmux::WorkspaceKey::for_session(workspace_project_id, task_key, session_id).name();

    tracing::info!(
        caller = "tauri",
        shared_launch_service = true,
        target = "rmux",
        cwd = %launch_result.cwd.display(),
        command_label = %launch_result.command_label,
        workspace = %workspace,
        "session created via shared launch service"
    );

    let session_id = session_id.to_string();
    crate::commands::blocking(move || {
        let cwd = launch_result.cwd.to_string_lossy().into_owned();
        let env = launch_result
            .env
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let argv = std::iter::once(launch_result.program.clone())
            .chain(launch_result.args.iter().cloned())
            .collect();
        crate::rmux_ops::spawn_resource_blocking(
            &session_id,
            &session_id,
            &workspace,
            argv,
            &cwd,
            &env,
        )
        .map(|_| ())
    })
    .await
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

    use super::{is_daemon_connection_error, require_task_key, rollback_branch_creation};

    #[test]
    fn only_a_lost_daemon_connection_reads_as_a_daemon_crash() {
        assert!(is_daemon_connection_error(
            "write failed: Broken pipe (os error 32)"
        ));
        assert!(is_daemon_connection_error(
            "connect: No such file or directory"
        ));
        assert!(!is_daemon_connection_error("plugin chat is not running"));
    }

    #[test]
    fn user_launch_requires_a_nonempty_task_key() {
        assert_eq!(
            require_task_key(Some("PLA-315".to_string())).unwrap(),
            "PLA-315"
        );
        assert!(require_task_key(None).is_err());
        assert!(require_task_key(Some("  ".to_string())).is_err());
    }

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
