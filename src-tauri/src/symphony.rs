use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use planeai_core::orchestrator::{AutoProject, OrchestratorConfig};
use planeai_core::session::{Backend, DispatchConfig, NewSession};
use planeai_core::task::{LifecycleHook, TaskManagerConfig};
use planeai_core::template;

use crate::config::{self, Config};

// ─── SymphonyState (managed as Tauri state) ───

pub struct SymphonyState {
    pub token: Option<CancellationToken>,
    pub handle: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl SymphonyState {
    pub fn new() -> Self {
        Self {
            token: None,
            handle: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.token
            .as_ref()
            .map(|t| !t.is_cancelled())
            .unwrap_or(false)
    }
}

// ─── TauriBackend ───

pub struct TauriBackend {
    pub db: Arc<Mutex<Connection>>,
    pub app_handle: tauri::AppHandle,
    #[allow(dead_code)]
    pub notify_socket: std::path::PathBuf,
}

impl Backend for TauriBackend {
    fn create_worktree(
        &self,
        repo: &str,
        path: &str,
        branch: &str,
        base: &str,
    ) -> Result<(), String> {
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

    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        _session_id: &str,
    ) -> Result<(), String> {
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
        let conn = self.db.lock().map_err(|e| e.to_string())?;
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

    fn run_move_task(
        &self,
        config: &TaskManagerConfig,
        key: &str,
        status: &str,
        cwd: &Path,
    ) -> Result<(), String> {
        let mut vars = HashMap::new();
        vars.insert("key", key);
        vars.insert("status", status);
        let cmd_str = template::render(&config.move_task, &vars);
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }
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

    fn notify_gui(&self, _session_id: &str) -> Result<(), String> {
        let _ = self.app_handle.emit("sessions-changed", ());
        Ok(())
    }

    fn kill_session(&self, session: &NewSession) -> Result<(), String> {
        if let Some(tmux_name) = &session.tmux_name {
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", tmux_name])
                .output();
        }
        let conn = self.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE sessions SET status = 'exited' WHERE id = ?1",
            params![session.id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        let conn = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, tmux_name, branch, worktree_path, provider, backend, auto_approve, task_key, base_branch
             FROM sessions WHERE status = 'active' AND auto_dispatched = 1"
        ).map_err(|e| e.to_string())?;
        let sessions = stmt
            .query_map([], |row| {
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
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(sessions)
    }
}

// ─── Config → OrchestratorConfig conversion ───

struct Project {
    id: String,
    name: String,
    path: String,
    task_manager: Option<String>,
}

pub fn build_orchestrator_config(
    config: &Config,
    db: &Connection,
    socket_path: std::path::PathBuf,
) -> Option<OrchestratorConfig> {
    let projects = load_auto_projects(db);
    if projects.is_empty() {
        return None;
    }

    let default_tm_name = config
        .default_task_manager
        .as_deref()
        .or_else(|| config.task_managers.keys().next().map(|s| s.as_str()))?;

    let backend_str = config::resolve_backend(config);
    let home = config::home_dir();

    let mut auto_projects = Vec::new();
    let mut poll_interval_ms = 30000u64;
    let mut max_concurrent = 3usize;

    for project in &projects {
        let tm_name = project.task_manager.as_deref().unwrap_or(default_tm_name);
        let tm = config.task_managers.get(tm_name)?;
        let auto = tm.auto_dispatch.as_ref()?;

        poll_interval_ms = auto.poll_interval_ms;
        max_concurrent = auto.max_concurrent;

        let provider_key = auto.provider.as_deref().unwrap_or(&config.default_provider);
        let provider = config.providers.get(provider_key)?;

        let terminal_states = auto
            .terminal_states
            .clone()
            .unwrap_or_else(|| vec!["done".into(), "cancelled".into(), "canceled".into()]);

        auto_projects.push(AutoProject {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            project_path: project.path.clone(),
            task_manager_config: TaskManagerConfig {
                list_tasks: tm.list_tasks.clone(),
                get_task: tm.get_task.clone(),
                move_task: tm.move_task.clone(),
                terminal_states,
                on_start: tm.on_start.as_ref().map(|h| LifecycleHook {
                    move_to: h.move_to.clone(),
                }),
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

    Some(OrchestratorConfig {
        poll_interval_ms,
        max_concurrent,
        socket_path,
        projects: auto_projects,
    })
}

fn load_auto_projects(conn: &Connection) -> Vec<Project> {
    let mut stmt = match conn.prepare(
        "SELECT id, name, path, task_manager FROM projects WHERE status = 'active' AND auto_mode = 1"
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            task_manager: row.get(3)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}
