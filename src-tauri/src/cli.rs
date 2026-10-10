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
    /// Name or id of the project owning `task_key`; defaults to `project`.
    pub task_project: Option<String>,
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
    /// Project owning `task_key` when it differs from `project_id`.
    pub task_project_id: Option<String>,
    pub parent_session_id: Option<String>,
    /// The task prompt, part of `command`. A `local` session runs no command here: its
    /// first attach in the app spawns the agent with this prompt.
    pub prompt: Option<String>,
}

impl SessionPlan {
    fn workspace_project_id(&self) -> &str {
        self.task_project_id.as_deref().unwrap_or(&self.project_id)
    }
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

    if let Some(prompt) = &opts.prompt {
        planeai_core::session_launch::check_task_prompt(prompt)?;
    }
    let cmd = provider_def.first_launch_command(opts.yolo, opts.prompt.as_deref());

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
        task_project_id: None,
        parent_session_id: opts.parent_session_id.clone(),
        prompt: opts.prompt.clone(),
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

        crate::session_ops::agent_spawn(
            &plan.session_id,
            &plan.command,
            &effective_working_dir,
            &env.config.resolved_extra_path_dirs(),
            env.config.wsl_target(),
        )?
        .in_daemon(&plan.session_id)?;
    } else if plan.backend == planeai_rmux::BACKEND {
        // Without this branch the session row would be written with no process
        // behind it, leaving a session that looks active and is empty.
        let workspace = planeai_rmux::WorkspaceKey::for_session(
            plan.workspace_project_id(),
            plan.task_key.as_deref(),
            &plan.session_id,
        )
        .name();
        crate::session_ops::agent_spawn(
            &plan.session_id,
            &plan.command,
            &effective_working_dir,
            &env.config.resolved_extra_path_dirs(),
            env.config.wsl_target(),
        )?
        .in_rmux(&plan.session_id, &workspace)?;
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

    // Store worktree_path so gates and agents know where to run.
    // For redirected sessions (branch already in another worktree), store that path.
    // For worktree-created sessions, store the new worktree path.
    // Cleanup guards against deleting non-loop-managed worktrees via branch name check.
    let worktree_path = match &plan.branch_strategy {
        BranchStrategy::Checkout { repo, .. } | BranchStrategy::Worktree { repo, .. }
            if was_redirected =>
        {
            git::linked_worktree_path(repo, &effective_working_dir)
        }
        BranchStrategy::Worktree { path, .. } => Some(path.clone()),
        BranchStrategy::Checkout { .. } => None,
    };
    let session = db::create_session_with_params(
        conn,
        &planeai_core::services::CreateSessionParams {
            id: plan.session_id.clone(),
            project_id: plan.project_id.clone(),
            name: plan.session_name.clone(),
            tmux_name: plan.tmux_name.clone(),
            branch: plan.branch.clone(),
            worktree_path,
            worktree_owned: Some(true),
            provider: Some(plan.provider.clone()),
            backend: plan.backend.clone(),
            auto_approve: plan.yolo,
            task_key: plan.task_key.clone(),
            task_project_id: plan.task_project_id.clone(),
            base_branch: plan.base_branch.clone(),
            parent_session_id: plan.parent_session_id.clone(),
            pending_prompt: crate::session_ops::pending_prompt(&plan.backend, plan.prompt.clone()),
            ..Default::default()
        },
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
    proj.require_folder()?;

    let task_project_id = resolve_task_project_id(&projects, proj, &opts)?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let mut plan = build_session_plan(&session_id, &opts, &env, proj)?;
    plan.task_project_id = task_project_id;

    execute_plan(&plan, conn, &env)?;

    db::get_session(conn, &session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "session created but not found in database".to_string())
}

/// Resolve `--task-project` to a project id; None when it names the session's own project.
fn resolve_task_project_id(
    projects: &[db::Project],
    session_project: &db::Project,
    opts: &SessionCreateOpts,
) -> Result<Option<String>, String> {
    let Some(task_project) = &opts.task_project else {
        return Ok(None);
    };
    if opts.task_key.is_none() {
        return Err("--task-project requires --task-key".to_string());
    }
    let owner = projects
        .iter()
        .find(|p| p.id == *task_project || p.name == *task_project)
        .ok_or_else(|| format!("unknown task project: {task_project}"))?;
    Ok((owner.id != session_project.id).then(|| owner.id.clone()))
}

/// Options for a sibling session in the invoking agent's TaskWorkspace.
pub struct WorkspaceSiblingOpts {
    pub name: Option<String>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub yolo: bool,
    pub provider: Option<String>,
    pub prompt: Option<String>,
}

/// Create an isolated, task-linked sibling session for an existing agent session.
pub fn create_workspace_sibling(
    conn: &Connection,
    parent_session_id: &str,
    opts: WorkspaceSiblingOpts,
) -> Result<db::Session, String> {
    let parent = db::get_session(conn, parent_session_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("current session not found: {parent_session_id}"))?;
    let task_key = parent
        .task_key
        .clone()
        .ok_or_else(|| "current session is not linked to a task workspace".to_string())?;
    let project = db::list_projects(conn)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|candidate| candidate.id == parent.project_id)
        .ok_or_else(|| "project for current session not found".to_string())?;
    let suffix = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8];
    let branch = opts
        .branch
        .unwrap_or_else(|| format!("{}--{suffix}", parent.branch));

    create_session(
        conn,
        SessionCreateOpts {
            project: project.name,
            branch,
            name: opts.name.or_else(|| Some(parent.name.clone())),
            new_branch: true,
            worktree: true,
            base_branch: opts.base_branch.or(parent.base_branch.clone()),
            yolo: opts.yolo,
            provider: opts.provider.or(parent.provider.clone()),
            task_key: Some(task_key),
            task_project: parent.task_project_id.clone(),
            prompt: opts.prompt,
            parent_session_id: Some(parent.id),
        },
    )
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
            path_missing: false,
        }
    }

    fn task_opts(task_key: Option<&str>, task_project: Option<&str>) -> SessionCreateOpts {
        SessionCreateOpts {
            project: "myapp".to_string(),
            branch: "feat-x".to_string(),
            name: None,
            new_branch: true,
            worktree: false,
            base_branch: None,
            yolo: false,
            provider: None,
            task_key: task_key.map(str::to_string),
            task_project: task_project.map(str::to_string),
            prompt: None,
            parent_session_id: None,
        }
    }

    #[test]
    fn task_project_resolves_by_name_or_id_and_ignores_the_own_project() {
        let own = test_project();
        let owner = db::Project {
            id: "proj-2".to_string(),
            name: "owner".to_string(),
            ..test_project()
        };
        let projects = [own.clone(), owner];
        let resolve = |task_project| {
            resolve_task_project_id(&projects, &own, &task_opts(Some("OWN-1"), task_project))
        };

        assert_eq!(resolve(Some("owner")).unwrap().as_deref(), Some("proj-2"));
        assert_eq!(resolve(Some("proj-2")).unwrap().as_deref(), Some("proj-2"));
        assert_eq!(resolve(Some("myapp")).unwrap(), None);
        assert_eq!(resolve(None).unwrap(), None);
        assert_eq!(
            resolve(Some("nope")).unwrap_err(),
            "unknown task project: nope"
        );
        assert_eq!(
            resolve_task_project_id(&projects, &own, &task_opts(None, Some("owner"))).unwrap_err(),
            "--task-project requires --task-key"
        );
    }

    #[test]
    fn a_local_session_keeps_its_prompt_for_the_apps_first_attach() {
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path().canonicalize().unwrap().display().to_string();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        ] {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo_path)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        }
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let project = db::create_project(&conn, "myapp", &repo_path).unwrap();
        let opts = SessionCreateOpts {
            prompt: Some("Fix the login redirect".to_string()),
            ..task_opts(Some("MYA-1"), None)
        };
        let env = Env {
            socket_path: repo.path().join("no-gui.sock"),
            ..test_env("local")
        };
        let plan = build_session_plan("session-1", &opts, &env, &project).unwrap();

        execute_plan(&plan, &conn, &env).unwrap();

        // The app's first attach spawns the agent with it (`resolve_attach_plan`).
        assert_eq!(
            db::pending_prompt(&conn, "session-1").unwrap().as_deref(),
            Some("Fix the login redirect")
        );
    }

    #[test]
    fn the_cli_starts_an_agent_with_the_command_the_app_uses() {
        let env = test_env("daemon");
        let command = |yolo: bool, prompt: Option<&str>| {
            let opts = SessionCreateOpts {
                yolo,
                prompt: prompt.map(str::to_string),
                ..task_opts(Some("MYA-1"), None)
            };
            let plan = build_session_plan("session-1", &opts, &env, &test_project()).unwrap();
            let app = env.config.providers["kiro"].first_launch_command(yolo, prompt);
            (plan.command, app)
        };
        for (yolo, prompt) in [(false, None), (true, Some("- Fix it")), (false, Some(""))] {
            let (cli, app) = command(yolo, prompt);
            assert_eq!(cli, app, "yolo {yolo}, prompt {prompt:?}");
        }
        assert_eq!(command(false, Some("")).0, "kiro-cli chat");
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
            task_project: None,
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
            task_project: None,
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
            task_project: None,
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
            task_project: None,
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
            task_project: None,
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
