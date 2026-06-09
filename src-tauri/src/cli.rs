use rusqlite::Connection;

use crate::db;

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
    fn checkout_branch(&self, repo: &str, branch: &str, new: bool, base: Option<&str>) -> Result<(), String>;
    fn create_worktree(&self, repo: &str, path: &str, branch: &str, base: &str) -> Result<(), String>;
    fn create_tmux_session(&self, name: &str, cwd: &str, cmd: &str, session_id: &str) -> Result<(), String>;
}

pub struct NoOpBackend;

impl Backend for NoOpBackend {
    fn checkout_branch(&self, _repo: &str, _branch: &str, _new: bool, _base: Option<&str>) -> Result<(), String> { Ok(()) }
    fn create_worktree(&self, _repo: &str, _path: &str, _branch: &str, _base: &str) -> Result<(), String> { Ok(()) }
    fn create_tmux_session(&self, _name: &str, _cwd: &str, _cmd: &str, _session_id: &str) -> Result<(), String> { Ok(()) }
}

pub struct Env {
    pub backend: String,
    pub socket_path: std::path::PathBuf,
}

pub fn run_session_create(conn: &Connection, opts: &SessionCreateOpts, backend: &dyn Backend) -> Result<String, String> {
    let env = Env {
        backend: "tmux".to_string(),
        socket_path: crate::paths::notify_socket_path(),
    };
    run_session_create_with_env(conn, opts, backend, &env)
}

pub fn run_session_create_with_env(conn: &Connection, opts: &SessionCreateOpts, backend: &dyn Backend, env: &Env) -> Result<String, String> {
    if env.backend == "direct" && !env.socket_path.exists() {
        return Err("GUI is not running (socket not found). Direct backend requires the GUI to spawn sessions.".to_string());
    }

    let projects = db::list_projects(conn).map_err(|e| e.to_string())?;
    let project = projects.iter().find(|p| p.name == opts.project)
        .ok_or_else(|| format!("unknown project: {}", opts.project))?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let short_id = &session_id.replace('-', "")[..8];

    let worktree_path = if opts.worktree {
        let base = opts.base_branch.as_deref().unwrap_or("main");
        let home = crate::config::home_dir();
        let wt_path = format!("{home}/.planeai/worktrees/{}/{short_id}", project.name);
        backend.create_worktree(&project.path, &wt_path, &opts.branch, base)?;
        Some(wt_path)
    } else {
        backend.checkout_branch(&project.path, &opts.branch, opts.new_branch, opts.base_branch.as_deref())?;
        None
    };

    let working_dir = worktree_path.as_deref().unwrap_or(&project.path);

    let tmux_name = if env.backend == "tmux" {
        let tn = crate::tmux::session_name(&project.name);
        backend.create_tmux_session(&tn, working_dir, "", &session_id)?;
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
        opts.provider.as_deref(),
        &env.backend,
        opts.yolo,
        opts.task_key.as_deref(),
        opts.base_branch.as_deref(),
    ).map_err(|e| e.to_string())?;

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
