use rusqlite::Connection;

use crate::{config, db, template};

/// Shell-escape a string by wrapping in single quotes, escaping any internal single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

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
}

pub trait Backend {
    fn checkout_branch(
        &self,
        repo: &str,
        branch: &str,
        new: bool,
        base: Option<&str>,
    ) -> Result<(), String>;
    fn create_worktree(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        base: &str,
    ) -> Result<(), String>;
    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
    ) -> Result<(), String>;
}

pub struct NoOpBackend;

impl Backend for NoOpBackend {
    fn checkout_branch(
        &self,
        _repo: &str,
        _branch: &str,
        _new: bool,
        _base: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }
    fn create_worktree(
        &self,
        _repo: &str,
        _path: &str,
        _branch: &str,
        _base: &str,
    ) -> Result<(), String> {
        Ok(())
    }
    fn create_tmux_session(
        &self,
        _name: &str,
        _cwd: &str,
        _cmd: &str,
        _session_id: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub struct Env {
    pub backend: String,
    pub socket_path: std::path::PathBuf,
    pub config: config::Config,
}

pub fn run_session_create(
    conn: &Connection,
    opts: &SessionCreateOpts,
    backend: &dyn Backend,
) -> Result<String, String> {
    let cfg_dir = config::config_dir("planeai");
    let (cfg, _) = config::load(&cfg_dir);
    let env = Env {
        backend: config::resolve_backend(&cfg).to_string(),
        socket_path: crate::paths::notify_socket_path(),
        config: cfg,
    };
    run_session_create_with_env(conn, opts, backend, &env)
}

pub fn run_session_create_with_env(
    conn: &Connection,
    opts: &SessionCreateOpts,
    backend: &dyn Backend,
    env: &Env,
) -> Result<String, String> {
    if env.backend == "direct" && !env.socket_path.exists() {
        return Err("GUI is not running (socket not found). Direct backend requires the GUI to spawn sessions.".to_string());
    }

    let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
    let project = projects
        .iter()
        .find(|p| p.name == opts.project)
        .ok_or_else(|| format!("unknown project: {}", opts.project))?;

    // Resolve provider and build launch command
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
        let mut vars = std::collections::HashMap::new();
        vars.insert("prompt", prompt.as_str());
        let rendered = template::render(prompt_tpl, &vars);
        let escaped = shell_escape(&rendered);
        cmd = format!("{cmd} {escaped}");
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let short_id = &session_id.replace('-', "")[..8];

    let worktree_path = if opts.worktree {
        let base = opts.base_branch.as_deref().unwrap_or("main");
        let home = crate::config::home_dir();
        let wt_path = format!("{home}/.planeai/worktrees/{}/{short_id}", project.name);
        backend.create_worktree(&project.path, &wt_path, &opts.branch, base)?;
        Some(wt_path)
    } else {
        backend.checkout_branch(
            &project.path,
            &opts.branch,
            opts.new_branch,
            opts.base_branch.as_deref(),
        )?;
        None
    };

    let working_dir = worktree_path.as_deref().unwrap_or(&project.path);

    let tmux_name = if env.backend == "tmux" {
        let tn = crate::tmux::session_name(&project.name);
        backend.create_tmux_session(&tn, working_dir, &cmd, &session_id)?;
        Some(tn)
    } else {
        None
    };

    let session_name = opts.name.as_deref().unwrap_or(&opts.branch);

    let session = db::create_session_with_id(
        conn,
        &session_id,
        &project.id,
        session_name,
        tmux_name.as_deref(),
        &opts.branch,
        worktree_path.as_deref(),
        Some(provider_key),
        &env.backend,
        opts.yolo,
        opts.task_key.as_deref(),
        opts.base_branch.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    // Notify the GUI via socket (fire-and-forget)
    if env.socket_path.exists() {
        let _ = notify_gui(&env.socket_path, &session_id);
    }

    serde_json::to_string(&session).map_err(|e| e.to_string())
}

fn notify_gui(socket_path: &std::path::Path, session_id: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path).map_err(|e| e.to_string())?;
    let msg = format!("{{\"event\":\"session_created\",\"session_id\":\"{session_id}\"}}\n");
    stream.write_all(msg.as_bytes()).map_err(|e| e.to_string())
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
        let mut vars = std::collections::HashMap::new();
        vars.insert("prompt", prompt.as_str());
        let rendered = template::render(prompt_tpl, &vars);
        let escaped = shell_escape(&rendered);
        cmd = format!("{cmd} {escaped}");
    }

    let short_id = &session_id.replace('-', "")[..8];

    let branch_strategy = if opts.worktree {
        let base = opts.base_branch.as_deref().unwrap_or("main").to_string();
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
        None
    };

    let session_name = opts
        .name
        .as_deref()
        .unwrap_or(&opts.branch)
        .to_string();

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
    })
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
        };

        let plan = build_session_plan("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", &opts, &test_env("tmux"), &test_project()).unwrap();

        assert_eq!(plan.branch_strategy, BranchStrategy::Checkout {
            repo: "/home/user/myapp".to_string(),
            branch: "feat-x".to_string(),
            new: true,
            base: Some("main".to_string()),
        });
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
        };

        let plan = build_session_plan("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", &opts, &test_env("tmux"), &test_project()).unwrap();

        let home = config::home_dir();
        assert_eq!(plan.branch_strategy, BranchStrategy::Worktree {
            repo: "/home/user/myapp".to_string(),
            path: format!("{home}/.planeai/worktrees/myapp/aaaaaaaa"),
            branch: "feat-wt".to_string(),
            base: "develop".to_string(),
        });
        assert_eq!(plan.working_dir, format!("{home}/.planeai/worktrees/myapp/aaaaaaaa"));
        assert_eq!(plan.session_name, "wt-session");
    }

    #[test]
    fn plan_direct_backend_has_no_tmux_name() {
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
        };

        let plan = build_session_plan("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", &opts, &test_env("direct"), &test_project()).unwrap();

        assert_eq!(plan.tmux_name, None);
        assert_eq!(plan.backend, "direct");
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
        };

        let plan = build_session_plan("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", &opts, &test_env("tmux"), &test_project()).unwrap();

        // Default config has claude with --dangerously-skip-permissions
        assert!(plan.command.contains("--dangerously-skip-permissions") || plan.command.contains("--trust-all-tools"));
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
        };

        let result = build_session_plan("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", &opts, &test_env("tmux"), &test_project());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown provider"));
    }
}
