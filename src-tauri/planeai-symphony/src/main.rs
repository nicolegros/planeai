use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use planeai_core::orchestrator::{AutoProject, Orchestrator, OrchestratorConfig};
use planeai_core::session::{Backend, DispatchConfig, NewSession};
use planeai_core::task::{LifecycleHook, TaskManagerConfig};
use planeai_core::template;
use rusqlite::{params, Connection};
use serde::Deserialize;

// ─── Config types (subset of planeai-app config, just what we need) ───

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    providers: HashMap<String, Provider>,
    #[serde(default = "default_provider")]
    default_provider: String,
    #[serde(default)]
    task_managers: HashMap<String, TaskManagerDef>,
    #[serde(default)]
    session_backend: Option<String>,
}

fn default_provider() -> String { "kiro".to_string() }

#[derive(Deserialize)]
struct Provider {
    command: String,
    #[serde(default)]
    yolo_flag: Option<String>,
    #[serde(default)]
    prompt_command: Option<String>,
}

#[derive(Deserialize)]
struct TaskManagerDef {
    get_task: String,
    move_task: String,
    list_tasks: String,
    #[serde(default)]
    auto_dispatch: Option<AutoDispatchDef>,
    #[serde(default)]
    on_start: Option<HookDef>,
    #[serde(default)]
    templates: Option<TemplatesDef>,
}

#[derive(Deserialize)]
struct AutoDispatchDef {
    #[serde(default = "default_poll")]
    poll_interval_ms: u64,
    #[serde(default = "default_max_concurrent")]
    max_concurrent: usize,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    terminal_states: Option<Vec<String>>,
}

fn default_poll() -> u64 { 30000 }
fn default_max_concurrent() -> usize { 3 }

#[derive(Deserialize)]
struct HookDef { move_to: String }

#[derive(Deserialize)]
struct TemplatesDef {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

// ─── DB types ───

struct Project {
    id: String,
    name: String,
    path: String,
    auto_mode: bool,
    task_manager: Option<String>,
}

// ─── Real Backend ───

struct RealBackend {
    db_path: PathBuf,
    notify_socket: PathBuf,
}

impl Backend for RealBackend {
    fn create_worktree(&self, repo: &str, path: &str, branch: &str, base: &str) -> Result<(), String> {
        let output = Command::new("git")
            .args(["worktree", "add", "-b", branch, path, base])
            .current_dir(repo)
            .output()
            .map_err(|e| format!("git worktree add: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    fn create_tmux_session(&self, name: &str, cwd: &str, cmd: &str, _session_id: &str) -> Result<(), String> {
        let output = Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "-c", cwd, cmd])
            .output()
            .map_err(|e| format!("tmux new-session: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        let conn = Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        let created_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO sessions (id, project_id, name, tmux_name, branch, status, created_at, worktree_path, provider, backend, auto_approve, task_key, base_branch, auto_dispatched)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1)",
            params![
                session.id, session.project_id, session.name,
                session.tmux_name, session.branch, created_at,
                session.worktree_path, session.provider, session.backend,
                session.auto_approve, session.task_key, session.base_branch,
            ],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn run_move_task(&self, config: &TaskManagerConfig, key: &str, status: &str, cwd: &Path) -> Result<(), String> {
        let mut vars = HashMap::new();
        vars.insert("key", key);
        vars.insert("status", status);
        let cmd_str = template::render(&config.move_task, &vars);
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() { return Ok(()); }
        let output = Command::new(parts[0])
            .args(&parts[1..])
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("move_task: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    fn notify_gui(&self, session_id: &str) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        if !self.notify_socket.exists() { return Ok(()); }
        let mut stream = UnixStream::connect(&self.notify_socket).map_err(|e| e.to_string())?;
        let msg = format!("{{\"event\":\"session_created\",\"session_id\":\"{session_id}\"}}\n");
        stream.write_all(msg.as_bytes()).map_err(|e| e.to_string())
    }

    fn kill_session(&self, session: &NewSession) -> Result<(), String> {
        // Kill tmux session if it exists
        if let Some(tmux_name) = &session.tmux_name {
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", tmux_name])
                .output();
        }
        // Mark session as exited in DB
        let conn = Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE sessions SET status = 'exited' WHERE id = ?1",
            params![session.id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        let conn = Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, tmux_name, branch, worktree_path, provider, backend, auto_approve, task_key, base_branch
             FROM sessions WHERE status = 'active' AND auto_dispatched = 1"
        ).map_err(|e| e.to_string())?;
        let sessions = stmt.query_map([], |row| {
            Ok(NewSession {
                id: row.get(0)?,
                project_id: row.get(1)?,
                project_name: String::new(),
                name: row.get(2)?,
                tmux_name: row.get(3)?,
                branch: row.get(4)?,
                worktree_path: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                provider: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                backend: row.get(7)?,
                auto_approve: row.get::<_, i64>(8)? != 0,
                task_key: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                base_branch: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                auto_dispatched: true,
                command: String::new(),
            })
        }).map_err(|e| e.to_string())?
          .filter_map(|r| r.ok())
          .collect();
        Ok(sessions)
    }
}

// ─── Main ───

fn load_config(config_dir: &Path) -> Result<Config, String> {
    let path = config_dir.join("config.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let stripped = json_comments::StripComments::new(content.as_bytes());
    serde_json::from_reader(stripped)
        .map_err(|e| format!("cannot parse config.json: {e}"))
}

fn load_projects(db_path: &Path) -> Result<Vec<Project>, String> {
    eprintln!("planeai-symphony: opening db at {}", db_path.display());
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    // Ensure auto_mode and task_manager columns exist
    let _ = conn.execute_batch("ALTER TABLE projects ADD COLUMN auto_mode INTEGER NOT NULL DEFAULT 0");
    let _ = conn.execute_batch("ALTER TABLE projects ADD COLUMN task_manager TEXT");

    // Debug: show all projects
    let mut debug_stmt = conn.prepare("SELECT id, name, auto_mode, status FROM projects").map_err(|e| e.to_string())?;
    let debug_rows: Vec<String> = debug_stmt.query_map([], |row| {
        Ok(format!("  {} auto_mode={} status={}", row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?))
    }).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    eprintln!("planeai-symphony: all projects:\n{}", debug_rows.join("\n"));

    let mut stmt = conn.prepare(
        "SELECT id, name, path, auto_mode, task_manager FROM projects WHERE status = 'active' AND auto_mode = 1"
    ).map_err(|e| e.to_string())?;
    let projects = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            auto_mode: row.get::<_, i64>(3)? != 0,
            task_manager: row.get(4)?,
        })
    }).map_err(|e| e.to_string())?
      .filter_map(|r| r.ok())
      .collect();
    Ok(projects)
}

fn resolve_backend_str(config: &Config) -> &str {
    match &config.session_backend {
        Some(b) => b.as_str(),
        None => if Command::new("which").arg("tmux").output().map(|o| o.status.success()).unwrap_or(false) { "tmux" } else { "direct" },
    }
}

fn build_orchestrator_config(config: &Config, projects: &[Project], socket_path: PathBuf) -> Result<OrchestratorConfig, String> {
    let default_tm_name = config.task_managers.keys().next()
        .ok_or("no task_managers configured")?;

    let mut auto_projects = Vec::new();
    let mut poll_interval_ms = 30000u64;
    let mut max_concurrent = 3usize;

    let backend_str = resolve_backend_str(config);

    for project in projects {
        let tm_name = project.task_manager.as_deref().unwrap_or(default_tm_name);
        let tm = config.task_managers.get(tm_name)
            .ok_or_else(|| format!("task_manager '{}' not found in config", tm_name))?;

        let auto = tm.auto_dispatch.as_ref()
            .ok_or_else(|| format!("task_manager '{}' has no auto_dispatch config", tm_name))?;

        poll_interval_ms = auto.poll_interval_ms;
        max_concurrent = auto.max_concurrent;

        let provider_key = auto.provider.as_deref()
            .unwrap_or(&config.default_provider);
        let provider = config.providers.get(provider_key)
            .ok_or_else(|| format!("provider '{}' not found", provider_key))?;

        let terminal_states = auto.terminal_states.clone()
            .unwrap_or_else(|| vec!["done".into(), "cancelled".into(), "canceled".into()]);

        let home = std::env::var("HOME").unwrap_or_default();

        auto_projects.push(AutoProject {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            project_path: project.path.clone(),
            task_manager_config: TaskManagerConfig {
                list_tasks: tm.list_tasks.clone(),
                get_task: tm.get_task.clone(),
                move_task: tm.move_task.clone(),
                terminal_states,
                on_start: tm.on_start.as_ref().map(|h| LifecycleHook { move_to: h.move_to.clone() }),
            },
            dispatch_config: DispatchConfig {
                provider: provider_key.to_string(),
                provider_command: provider.command.clone(),
                yolo: true,
                yolo_flag: provider.yolo_flag.clone(),
                worktree_root: format!("{home}/.planeai/worktrees"),
                base_branch: "main".to_string(),
                session_backend: backend_str.to_string(),
                prompt_template: tm.templates.as_ref().and_then(|t| t.prompt.clone()),
                name_template: tm.templates.as_ref().and_then(|t| t.name.clone()),
            },
        });
    }

    Ok(OrchestratorConfig {
        poll_interval_ms,
        max_concurrent,
        socket_path,
        projects: auto_projects,
    })
}

#[tokio::main]
async fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{home}/.config"));
    let config_dir = PathBuf::from(config_dir).join("planeai");

    let config = match load_config(&config_dir) {
        Ok(c) => c,
        Err(e) => { eprintln!("planeai-symphony: {e}"); std::process::exit(1); }
    };

    let app_data = planeai_core::app_data_dir();
    let db_path = app_data.join("planeai.db");
    let socket_path = app_data.join("symphony.sock");

    let projects = match load_projects(&db_path) {
        Ok(p) => p,
        Err(e) => { eprintln!("planeai-symphony: failed to load projects: {e}"); std::process::exit(1); }
    };

    if projects.is_empty() {
        eprintln!("planeai-symphony: no projects with auto_mode enabled");
        std::process::exit(0);
    }

    let orch_config = match build_orchestrator_config(&config, &projects, socket_path) {
        Ok(c) => c,
        Err(e) => { eprintln!("planeai-symphony: {e}"); std::process::exit(1); }
    };

    let backend = Arc::new(RealBackend {
        db_path,
        notify_socket: planeai_core::notify_socket_path(),
    });

    eprintln!("planeai-symphony: starting ({} project(s), poll {}ms, max {})",
        orch_config.projects.len(), orch_config.poll_interval_ms, orch_config.max_concurrent);

    if let Err(e) = Orchestrator::new(orch_config, backend).run().await {
        eprintln!("planeai-symphony: {e}");
        std::process::exit(1);
    }
}
