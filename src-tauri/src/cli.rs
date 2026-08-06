use rusqlite::Connection;

use planeai_tasks::model::DEFAULT_BASE_BRANCH;

#[cfg(not(windows))]
use crate::tmux;
use crate::{config, db, git};

pub fn run_project_list(conn: &Connection) -> String {
    let projects = db::list_projects(conn).unwrap_or_default();
    serde_json::to_string(&projects).unwrap()
}

pub struct SessionCreateOpts {
    pub project: String,
    pub branch: String,
    pub name: Option<String>,
    pub new_branch: bool,
    pub worktree: bool,
    pub base_branch: Option<String>,
    pub yolo: bool,
    pub provider: Option<String>,
    pub task_key: Option<String>,
    pub prompt: Option<String>,
    pub parent_session_id: Option<String>,
}

pub struct Env {
    pub backend: String,
    pub socket_path: std::path::PathBuf,
    pub config: config::Config,
}

// ─── Plan / Execute ───

#[derive(Debug, Clone, PartialEq)]
pub enum BranchStrategy {
    Checkout {
        repo: String,
        branch: String,
        new: bool,
        base: Option<String>,
    },
    Worktree {
        repo: String,
        path: String,
        branch: String,
        base: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionPlan {
    pub session_id: String,
    pub session_name: String,
    pub branch: String,
    pub branch_strategy: BranchStrategy,
    pub working_dir: String,
    pub tmux_name: Option<String>,
    pub command: String,
    pub provider: String,
    pub backend: String,
    pub yolo: bool,
    pub task_key: Option<String>,
    pub base_branch: Option<String>,
    pub project_id: String,
    pub parent_session_id: Option<String>,
}

pub fn build_session_plan(
    session_id: &str,
    opts: &SessionCreateOpts,
    env: &Env,
    project: &db::Project,
) -> Result<SessionPlan, String> {
    let provider_key = opts
        .provider
        .as_deref()
        .unwrap_or(&env.config.default_provider);
    let provider_def = env
        .config
        .providers
        .get(provider_key)
        .ok_or_else(|| format!("unknown provider: {provider_key}"))?;

    let mut cmd = config::launch_command(provider_def, opts.yolo);

    if let (Some(prompt), Some(prompt_tpl)) = (&opts.prompt, &provider_def.prompt_command) {
        planeai_core::template::append_prompt(&mut cmd, prompt_tpl, prompt);
    }

    let short_id = &session_id.replace('-', "")[..8];

    let branch_strategy = if opts.worktree {
        let base = opts
            .base_branch
            .as_deref()
            .unwrap_or(DEFAULT_BASE_BRANCH)
            .to_string();
        let home = config::home_dir();
        let wt_path = format!("{home}/.planeai/worktrees/{}/{short_id}", project.name);
        BranchStrategy::Worktree {
            repo: project.path.clone(),
            path: wt_path,
            branch: opts.branch.clone(),
            base,
        }
    } else {
        BranchStrategy::Checkout {
            repo: project.path.clone(),
            branch: opts.branch.clone(),
            new: opts.new_branch,
            base: opts.base_branch.clone(),
        }
    };

    let working_dir = match &branch_strategy {
        BranchStrategy::Worktree { path, .. } => path.clone(),
        BranchStrategy::Checkout { repo, .. } => repo.clone(),
    };

    let tmux_name = if env.backend == "tmux" {
        let sanitized = project.name.replace(' ', "-").replace(['.', ':'], "");
        Some(format!("planeai-{sanitized}-{short_id}"))
    } else {
        None // daemon backend doesn't use tmux names
    };

    let session_name = opts.name.as_deref().unwrap_or(&opts.branch).to_string();

    Ok(SessionPlan {
        session_id: session_id.to_string(),
        session_name,
        branch: opts.branch.clone(),
        branch_strategy,
        working_dir,
        tmux_name,
        command: cmd,
        provider: provider_key.to_string(),
        backend: env.backend.clone(),
        yolo: opts.yolo,
        task_key: opts.task_key.clone(),
        base_branch: opts.base_branch.clone(),
        project_id: project.id.clone(),
        parent_session_id: opts.parent_session_id.clone(),
    })
}

/// Check if a git error indicates a worktree conflict.
fn is_worktree_conflict(e: &str) -> bool {
    e.contains("already checked out") || e.contains("already used by worktree")
}

/// Try to reuse an existing worktree where the branch is already checked out.
fn reuse_existing_worktree(
    repo: &str,
    branch: &str,
    original_err: String,
) -> Result<String, String> {
    match git::find_worktree_for_branch(repo, branch) {
        Some(wt_path) => {
            tracing::info!(
                branch = %branch,
                worktree = %wt_path,
                "branch already in worktree, reusing"
            );
            Ok(wt_path)
        }
        None => Err(original_err),
    }
}

pub fn execute_plan(plan: &SessionPlan, conn: &Connection, env: &Env) -> Result<String, String> {
    // Resolve working_dir — may differ from plan if branch lives in a worktree
    let effective_working_dir;
    let mut was_redirected = false;

    match &plan.branch_strategy {
        BranchStrategy::Checkout {
            repo,
            branch,
            new,
            base,
        } => match git::checkout_branch(repo, branch, *new, base.as_deref()) {
            Ok(()) => {
                effective_working_dir = plan.working_dir.clone();
            }
            Err(e) if !*new && is_worktree_conflict(&e) => {
                effective_working_dir = reuse_existing_worktree(repo, branch, e)?;
                was_redirected = true;
            }
            Err(e) => return Err(e),
        },
        BranchStrategy::Worktree {
            repo,
            path,
            branch,
            base,
        } => match git::worktree_add(repo, path, branch, base) {
            Ok(()) => {
                effective_working_dir = path.clone();
            }
            Err(e) if is_worktree_conflict(&e) => {
                effective_working_dir = reuse_existing_worktree(repo, branch, e)?;
                was_redirected = true;
            }
            Err(e) => return Err(e),
        },
    }

    if plan.backend == "daemon" {
        let socket_path = planeai_ipc::daemon_socket_path();
        let daemon_bin = resolve_daemon_binary();
        let scrollback = 1_048_576;
        crate::daemon::ensure_running(&daemon_bin, &socket_path, scrollback)?;

        let extra_path_dirs = env.config.resolved_extra_path_dirs();
        let mut path_buf = String::new();
        let session_env = planeai_core::command::build_daemon_env(
            &extra_path_dirs,
            &plan.session_id,
            &mut path_buf,
        );
        let (program, args) = planeai_core::command::shell_args(&plan.command);
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        crate::daemon::spawn_session(
            &plan.session_id,
            program,
            &args_refs,
            &effective_working_dir,
            Some(&session_env),
        )?;
    } else if let Some(tmux_name) = &plan.tmux_name {
        #[cfg(not(windows))]
        {
            let extra_path_dirs = env.config.resolved_extra_path_dirs();
            tmux::create_session_with_cmd_and_path(
                tmux_name,
                &effective_working_dir,
                &plan.command,
                &plan.session_id,
                &extra_path_dirs,
            )?;
        }
        #[cfg(windows)]
        let _ = tmux_name;
    }

    let session = db::create_session_with_id(
        conn,
        &plan.session_id,
        &plan.project_id,
        &plan.session_name,
        plan.tmux_name.as_deref(),
        &plan.branch,
        // Store worktree_path so gates and agents know where to run.
        // For redirected sessions (branch already in another worktree), store that path.
        // For worktree-created sessions, store the new worktree path.
        // Cleanup guards against deleting non-loop-managed worktrees via branch name check.
        if was_redirected {
            Some(effective_working_dir.as_str())
        } else {
            match &plan.branch_strategy {
                BranchStrategy::Worktree { path, .. } => Some(path.as_str()),
                BranchStrategy::Checkout { .. } => None,
            }
        },
        Some(&plan.provider),
        &plan.backend,
        plan.yolo,
        plan.task_key.as_deref(),
        plan.base_branch.as_deref(),
        plan.parent_session_id.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    if env.socket_path.exists() {
        tracing::debug!(session_id = %plan.session_id, "[DEBUG-lsr1] execute_plan: sending notify_gui");
        let notify_result = notify_gui(&env.socket_path, &plan.session_id);
        tracing::debug!(session_id = %plan.session_id, success = notify_result.is_ok(), "[DEBUG-lsr1] execute_plan: notify_gui done");
    } else {
        tracing::warn!(session_id = %plan.session_id, "[DEBUG-lsr1] execute_plan: socket_path does not exist, skipping notify_gui");
    }

    serde_json::to_string(&session).map_err(|e| e.to_string())
}

/// Shared session creation orchestration used by both the JSON CLI and the AXI
/// interface. Loads config, resolves the project, builds a plan, and executes it.
/// Returns the created `Session` on success.
pub fn create_session(conn: &Connection, opts: SessionCreateOpts) -> Result<db::Session, String> {
    let cfg_dir = config::config_dir("planeai");
    let (cfg, _) = config::load(&cfg_dir);
    let backend = config::resolve_backend(&cfg).to_string();

    let env = Env {
        backend,
        socket_path: planeai_paths::notify_socket_path(),
        config: cfg,
    };

    let project_name = opts.project.clone();
    let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
    let proj = projects
        .iter()
        .find(|p| p.name == project_name)
        .ok_or_else(|| format!("unknown project: {project_name}"))?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let plan = build_session_plan(&session_id, &opts, &env, proj)?;

    execute_plan(&plan, conn, &env)?;

    db::get_session(conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "session created but not found in database".to_string())
}

/// Resolve the daemon binary path. Checks /usr/local/bin first, then falls back
/// to the current executable's directory.
fn resolve_daemon_binary() -> std::path::PathBuf {
    let symlinked = std::path::Path::new("/usr/local/bin/planeai-daemon");
    if symlinked.exists() {
        return symlinked.to_path_buf();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("planeai-daemon");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    // Last resort: hope it's on PATH
    std::path::PathBuf::from("planeai-daemon")
}

#[cfg(not(windows))]
fn notify_gui(socket_path: &std::path::Path, session_id: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path).map_err(|e| e.to_string())?;
    let msg = format!("{{\"event\":\"session_created\",\"session_id\":\"{session_id}\"}}\n");
    stream.write_all(msg.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn notify_gui(_socket_path: &std::path::Path, _session_id: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env(backend: &str) -> Env {
        Env {
            backend: backend.to_string(),
            socket_path: std::path::PathBuf::from("/tmp/fake.sock"),
            config: config::Config::default(),
        }
    }

    fn test_project() -> db::Project {
        db::Project {
            id: "proj-1".to_string(),
            name: "myapp".to_string(),
            path: "/home/user/myapp".to_string(),
            status: "active".to_string(),
            prefix: "MYA".to_string(),
            hidden: false,
        }
    }

    #[test]
    fn plan_checkout_mode() {
        let opts = SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "feat-x".to_string(),
            name: None,
            new_branch: true,
            worktree: false,
            base_branch: Some("main".to_string()),
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
            parent_session_id: None,
        };

        let plan = build_session_plan(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            &opts,
            &test_env("tmux"),
            &test_project(),
        )
        .unwrap();

        assert_eq!(
            plan.branch_strategy,
            BranchStrategy::Checkout {
                repo: "/home/user/myapp".to_string(),
                branch: "feat-x".to_string(),
                new: true,
                base: Some("main".to_string()),
            }
        );
        assert_eq!(plan.working_dir, "/home/user/myapp");
        assert_eq!(plan.session_name, "feat-x");
        assert_eq!(plan.tmux_name, Some("planeai-myapp-aaaaaaaa".to_string()));
    }

    #[test]
    fn plan_worktree_mode() {
        let opts = SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "feat-wt".to_string(),
            name: Some("wt-session".to_string()),
            new_branch: false,
            worktree: true,
            base_branch: Some("develop".to_string()),
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
            parent_session_id: None,
        };

        let plan = build_session_plan(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            &opts,
            &test_env("tmux"),
            &test_project(),
        )
        .unwrap();

        match &plan.branch_strategy {
            BranchStrategy::Worktree {
                repo,
                path,
                branch,
                base,
            } => {
                assert_eq!(repo, "/home/user/myapp");
                assert!(
                    path.ends_with("/.planeai/worktrees/myapp/aaaaaaaa"),
                    "unexpected worktree path: {path}"
                );
                assert_eq!(branch, "feat-wt");
                assert_eq!(base, "develop");
            }
            other => panic!("expected Worktree, got {:?}", other),
        }
        assert!(
            plan.working_dir
                .ends_with("/.planeai/worktrees/myapp/aaaaaaaa"),
            "unexpected working_dir: {}",
            plan.working_dir
        );
        assert_eq!(plan.session_name, "wt-session");
    }

    #[test]
    fn plan_daemon_backend_has_no_tmux_name() {
        let opts = SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: None,
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: None,
            prompt: None,
            parent_session_id: None,
        };

        let plan = build_session_plan(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            &opts,
            &test_env("daemon"),
            &test_project(),
        )
        .unwrap();

        assert_eq!(plan.tmux_name, None);
        assert_eq!(plan.backend, "daemon");
    }

    #[test]
    fn plan_yolo_flag_included_in_command() {
        let opts = SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: None,
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: true,
            provider: None,
            task_key: None,
            prompt: None,
            parent_session_id: None,
        };

        let plan = build_session_plan(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            &opts,
            &test_env("tmux"),
            &test_project(),
        )
        .unwrap();

        // Default config has claude with --dangerously-skip-permissions
        assert!(
            plan.command.contains("--dangerously-skip-permissions")
                || plan.command.contains("--trust-all-tools")
        );
    }

    #[test]
    fn plan_unknown_provider_errors() {
        let opts = SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "main".to_string(),
            name: None,
            new_branch: false,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: Some("nonexistent".to_string()),
            task_key: None,
            prompt: None,
            parent_session_id: None,
        };

        let result = build_session_plan(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            &opts,
            &test_env("tmux"),
            &test_project(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown provider"));
    }
}
