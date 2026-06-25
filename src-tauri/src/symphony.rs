use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

use planeai_core::orchestrator::{AutoProject, OrchestratorConfig};
use planeai_core::session::{Backend, DispatchConfig, NewSession, OnStartHook};
use planeai_core::task::{Task, TaskSource};

use planeai_tasks::model::{Status, DEFAULT_BASE_BRANCH};
use planeai_tasks::provider::TaskProvider;
use planeai_tasks::sqlite::SqliteRepository;

use crate::config::{self, Config};

// ─── SymphonyState (managed as Tauri state) ───

#[allow(dead_code)]
pub struct RunningOrchestrator {
    pub token: CancellationToken,
    pub handle: tauri::async_runtime::JoinHandle<()>,
    pub command_tx: tokio::sync::mpsc::Sender<planeai_core::orchestrator::OrchestratorCommand>,
}

pub struct SymphonyState {
    pub running: Option<RunningOrchestrator>,
}

impl SymphonyState {
    pub fn new() -> Self {
        Self { running: None }
    }

    pub fn is_running(&self) -> bool {
        self.running
            .as_ref()
            .map(|r| !r.token.is_cancelled())
            .unwrap_or(false)
    }
}

/// Spawn a std::thread that listens on the Symphony IPC channel and forwards
/// commands to the orchestrator via the mpsc sender.
pub fn start_ipc_bridge(
    app_dir: &std::path::Path,
    tx: tokio::sync::mpsc::Sender<planeai_core::orchestrator::OrchestratorCommand>,
) {
    use std::io::{BufRead, BufReader, Write};

    let app_dir = app_dir.to_path_buf();
    std::thread::spawn(move || {
        let listener =
            match planeai::ipc::IpcListener::bind(planeai::ipc::Channel::Symphony, &app_dir) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[symphony-ipc] bind failed: {e}");
                    return;
                }
            };

        loop {
            let mut stream = match listener.accept() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[symphony-ipc] accept failed: {e}");
                    continue;
                }
            };

            let mut line = String::new();
            if BufReader::new(&mut stream).read_line(&mut line).is_err() {
                continue;
            }

            match line.trim() {
                "stop" => {
                    let _ = tx.blocking_send(planeai_core::orchestrator::OrchestratorCommand::Stop);
                    break;
                }
                "status" => {
                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                    if tx
                        .blocking_send(planeai_core::orchestrator::OrchestratorCommand::Status {
                            reply: reply_tx,
                        })
                        .is_ok()
                    {
                        if let Ok(response) = reply_rx.blocking_recv() {
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.write_all(b"\n");
                        }
                    }
                }
                _ => {}
            }
        }
    });
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
        let mut cmd = std::process::Command::new("git");
        cmd.args(["worktree", "add", "-b", branch, path, base])
            .current_dir(repo);
        planeai_core::command::no_window(&mut cmd);
        let output = cmd
            .output()
            .map_err(|e| format!("git worktree add: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn create_tmux_session(
        &self,
        name: &str,
        cwd: &str,
        cmd: &str,
        session_id: &str,
    ) -> Result<(), String> {
        crate::tmux::create_session_with_cmd_and_path(name, cwd, cmd, session_id, &[])
    }

    #[cfg(windows)]
    fn create_tmux_session(
        &self,
        _name: &str,
        _cwd: &str,
        _cmd: &str,
        _session_id: &str,
    ) -> Result<(), String> {
        Err("tmux is not supported on Windows".to_string())
    }

    fn create_daemon_session(&self, session_id: &str, cmd: &str, cwd: &str) -> Result<(), String> {
        let socket_path = planeai_ipc::daemon_socket_path();
        let daemon_bin = crate::paths::resolve_daemon_binary(&self.app_handle);
        let scrollback = 1_048_576;
        let extra_path_dirs = {
            let cfg_state = self.app_handle.state::<crate::state::ConfigState>();
            let cfg = cfg_state.0.lock().map_err(|e| e.to_string())?;
            cfg.resolved_extra_path_dirs()
        };

        crate::daemon::ensure_running(&daemon_bin, &socket_path, scrollback)?;

        let mut path_buf = String::new();
        let env =
            planeai_core::command::build_daemon_env(&extra_path_dirs, session_id, &mut path_buf);
        let (program, args) = planeai_core::command::shell_args(cmd);
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        crate::daemon::spawn_session(session_id, program, &args_refs, cwd, Some(&env))
    }

    fn insert_session(&self, session: &NewSession) -> Result<(), String> {
        let conn = self.db.lock().map_err(|e| e.to_string())?;
        crate::db::create_session_with_id(
            &conn,
            &session.id,
            &session.project_id,
            &session.name,
            session.tmux_name.as_deref(),
            &session.branch,
            Some(session.worktree_path.as_str()).filter(|s| !s.is_empty()),
            Some(session.provider.as_str()).filter(|s| !s.is_empty()),
            &session.backend,
            session.auto_approve,
            Some(session.task_key.as_str()).filter(|s| !s.is_empty()),
            Some(session.base_branch.as_str()).filter(|s| !s.is_empty()),
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE sessions SET auto_dispatched = 1 WHERE id = ?1",
            params![session.id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn notify_gui(&self, _session_id: &str) -> Result<(), String> {
        let _ = self.app_handle.emit("sessions-changed", ());
        Ok(())
    }

    fn kill_session(&self, session: &NewSession) -> Result<(), String> {
        #[cfg(not(windows))]
        if let Some(tmux_name) = &session.tmux_name {
            let _ = crate::tmux::kill_session(tmux_name);
        }
        let conn = self.db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE sessions SET status = 'exited' WHERE id = ?1",
            params![session.id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn fetch_base(&self, repo: &str, base: &str) -> Result<String, String> {
        crate::git::resolve_base_branch(repo, base)
    }

    fn list_claimed_task_keys(&self) -> Result<HashSet<String>, String> {
        let conn = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT task_key FROM sessions WHERE status IN ('active', 'exited') AND task_key IS NOT NULL AND task_key != ''",
            )
            .map_err(|e| e.to_string())?;
        let keys = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(keys)
    }

    fn list_active_sessions(&self) -> Result<Vec<NewSession>, String> {
        let conn = self.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, tmux_name, branch, worktree_path, provider, backend, auto_approve, task_key, base_branch
             FROM sessions WHERE status = 'active' AND task_key IS NOT NULL AND task_key != ''"
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

    fn reload_dispatch_config(&self, provider_key: &str) -> Option<DispatchConfig> {
        let cfg_state = self.app_handle.state::<crate::state::ConfigState>();
        let cfg = cfg_state.0.lock().ok()?;
        let provider = cfg.providers.get(provider_key)?;
        let home = config::home_dir();
        let backend_str = config::resolve_backend(&cfg);

        let tm = cfg.task_management.as_ref()?;
        let auto = tm.auto_dispatch.as_ref()?;
        let base_branch = auto
            .base_branch
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_string());

        Some(DispatchConfig {
            provider: provider_key.to_string(),
            provider_command: provider.command.clone(),
            yolo: true,
            yolo_flag: provider.yolo_flag.clone(),
            worktree_root: format!("{home}/.planeai/worktrees"),
            base_branch,
            session_backend: backend_str.to_string(),
            prompt_template: tm.templates.as_ref().and_then(|t| t.prompt.clone()),
            prompt_command: provider.prompt_command.clone(),
            prompt_wrapper: provider.autonomous_prompt_template.clone(),
            name_template: tm.templates.as_ref().and_then(|t| t.name.clone()),
        })
    }
}

// ─── SqliteTaskSource: adapts planeai-tasks SqliteRepository to TaskSource ───

/// Adapter implementing TaskSource using the internal planeai-tasks SqliteRepository.
pub struct SqliteTaskSource {
    repo: SqliteRepository,
    terminal_states: Vec<String>,
}

impl SqliteTaskSource {
    pub fn new(repo: SqliteRepository, terminal_states: Vec<String>) -> Self {
        Self {
            repo,
            terminal_states,
        }
    }
}

impl TaskSource for SqliteTaskSource {
    fn list_tasks(&self) -> Result<Vec<Task>, String> {
        let tasks = self
            .repo
            .list(planeai_tasks::model::ListFilter::default())
            .map_err(|e| e.to_string())?;
        tracing::debug!(count = tasks.len(), "listed tasks from internal provider");

        // Build parent→children map from parent_key relationships
        let mut children_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for t in &tasks {
            if let Some(ref parent) = t.parent_key {
                children_map
                    .entry(parent.clone())
                    .or_default()
                    .push(t.key.clone());
            }
        }

        Ok(tasks
            .into_iter()
            .map(|t| {
                let subtasks = children_map.remove(&t.key).unwrap_or_default();
                into_core_task(t, subtasks)
            })
            .collect())
    }

    fn get_task(&self, key: &str) -> Result<Task, String> {
        let task = self.repo.get(key).map_err(|e| e.to_string())?;
        Ok(into_core_task(task, vec![]))
    }

    fn move_task(&self, key: &str, status: &str) -> Result<(), String> {
        tracing::info!(task_key = %key, status = %status, "moving task via internal provider");
        let new_status =
            Status::parse(status).ok_or_else(|| format!("invalid status: {status}"))?;
        self.repo
            .update(
                key,
                planeai_tasks::model::UpdateParams {
                    status: Some(new_status),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn is_terminal(&self, status: &str) -> bool {
        self.terminal_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case(status))
    }
}

fn into_core_task(t: planeai_tasks::model::Task, subtasks: Vec<String>) -> Task {
    Task {
        key: t.key,
        title: t.title,
        status: t.status.as_str().to_string(),
        description: t.description,
        priority: t.priority,
        blocked_by: t.blocked_by,
        subtasks,
        base_branch: t.base_branch,
    }
}

// ─── Config → OrchestratorConfig conversion ───

struct Project {
    id: String,
    name: String,
    path: String,
    prefix: String,
}

pub fn build_orchestrator_config(config: &Config, db: &Connection) -> Option<OrchestratorConfig> {
    let projects = load_auto_projects(db);
    if projects.is_empty() {
        return None;
    }

    let tm = config.task_management.as_ref()?;

    let backend_str = config::resolve_backend(config);
    let home = config::home_dir();

    let mut auto_projects = Vec::new();
    let mut poll_interval_ms = 30000u64;
    let mut max_concurrent = 3usize;

    for project in &projects {
        let auto = tm.auto_dispatch.as_ref()?;

        poll_interval_ms = auto.poll_interval_ms;
        max_concurrent = auto.max_concurrent;

        let provider_key = auto.provider.as_deref().unwrap_or(&config.default_provider);
        let provider = config.providers.get(provider_key)?;

        let terminal_states = auto
            .terminal_states
            .clone()
            .unwrap_or_else(|| vec!["done".into(), "cancelled".into(), "canceled".into()]);

        // Build SqliteTaskSource from a new connection to the same DB
        let prefix = &project.prefix;
        let db_path = crate::paths::app_data_dir().join("planeai.db");
        let _ = std::fs::create_dir_all(db_path.parent().unwrap_or(std::path::Path::new(".")));
        let task_repo = match SqliteRepository::open(db_path.to_str().unwrap_or(""), prefix) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let task_source = Arc::new(SqliteTaskSource::new(task_repo, terminal_states));

        let on_start = tm.on_start.as_ref().map(|h| OnStartHook {
            move_to: h.move_to.clone(),
        });

        auto_projects.push(AutoProject {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            project_path: project.path.clone(),
            task_source,
            on_start,
            dispatch_config: DispatchConfig {
                provider: provider_key.to_string(),
                provider_command: provider.command.clone(),
                yolo: true,
                yolo_flag: provider.yolo_flag.clone(),
                worktree_root: format!("{home}/.planeai/worktrees"),
                base_branch: auto
                    .base_branch
                    .clone()
                    .unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_string()),
                session_backend: backend_str.to_string(),
                prompt_template: tm.templates.as_ref().and_then(|t| t.prompt.clone()),
                prompt_command: provider.prompt_command.clone(),
                prompt_wrapper: provider.autonomous_prompt_template.clone(),
                name_template: tm.templates.as_ref().and_then(|t| t.name.clone()),
            },
        });
    }

    tracing::info!(
        projects = auto_projects.len(),
        poll_interval_ms,
        max_concurrent,
        "built orchestrator config with internal task provider"
    );

    Some(OrchestratorConfig {
        poll_interval_ms,
        max_concurrent,
        projects: auto_projects,
    })
}

fn load_auto_projects(conn: &Connection) -> Vec<Project> {
    let mut stmt = match conn.prepare(
        "SELECT id, name, path, prefix FROM projects WHERE status = 'active' AND auto_mode = 1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            prefix: row.get(3)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AutoDispatchConfig, Config, LifecycleHook as ConfigLifecycleHook, Provider, TaskManager,
    };
    use std::collections::HashMap;

    fn minimal_config_with_auto_dispatch(base_branch: Option<String>) -> Config {
        let mut providers = HashMap::new();
        providers.insert(
            "kiro".to_string(),
            Provider {
                command: "kiro-cli chat".to_string(),
                yolo_flag: Some("--trust-all-tools".to_string()),
                prompt_command: None,
                autonomous_prompt_template: None,
                resume_command: None,
                session_id_pattern: None,
                resume_flag: None,
                list_sessions_command: None,
            },
        );

        Config {
            default_provider: "kiro".to_string(),
            task_management: Some(TaskManager {
                templates: None,
                on_start: Some(ConfigLifecycleHook {
                    move_to: "in_progress".to_string(),
                }),
                on_notify: None,
                on_restart: None,
                on_complete: None,
                on_pr_open: None,
                on_pr_merge: None,
                auto_dispatch: Some(AutoDispatchConfig {
                    poll_interval_ms: 30000,
                    max_concurrent: 2,
                    provider: Some("kiro".to_string()),
                    terminal_states: None,
                    base_branch,
                }),
            }),
            providers,
            ..Config::default()
        }
    }

    #[test]
    fn build_orchestrator_config_uses_auto_dispatch_base_branch_override() {
        let config = minimal_config_with_auto_dispatch(Some("develop".to_string()));

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, status, auto_mode) VALUES ('p1', 'myapp', '/repo', 'active', 1)",
            [],
        ).unwrap();

        let orch_config = build_orchestrator_config(&config, &conn);
        assert!(orch_config.is_some());

        let orch = orch_config.unwrap();
        assert_eq!(orch.projects[0].dispatch_config.base_branch, "develop");
    }

    #[test]
    fn build_orchestrator_config_falls_back_to_main_when_no_base_branch_override() {
        let config = minimal_config_with_auto_dispatch(None);

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, status, auto_mode) VALUES ('p1', 'myapp', '/repo', 'active', 1)",
            [],
        ).unwrap();

        let orch_config = build_orchestrator_config(&config, &conn);
        assert!(orch_config.is_some());

        let orch = orch_config.unwrap();
        assert_eq!(orch.projects[0].dispatch_config.base_branch, "main");
    }

    #[test]
    fn autonomous_prompt_template_populates_prompt_wrapper() {
        let mut config = minimal_config_with_auto_dispatch(None);
        config.providers.get_mut("kiro").unwrap().prompt_command = Some("{prompt}".to_string());
        config
            .providers
            .get_mut("kiro")
            .unwrap()
            .autonomous_prompt_template = Some("Be autonomous.\n{prompt}".to_string());

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, status, auto_mode) VALUES ('p1', 'myapp', '/repo', 'active', 1)",
            [],
        ).unwrap();

        let orch = build_orchestrator_config(&config, &conn).unwrap();

        assert_eq!(
            orch.projects[0].dispatch_config.prompt_wrapper,
            Some("Be autonomous.\n{prompt}".to_string()),
        );
        assert_eq!(
            orch.projects[0].dispatch_config.prompt_command,
            Some("{prompt}".to_string()),
        );
    }

    #[test]
    fn prompt_wrapper_is_none_when_autonomous_prompt_template_not_set() {
        let mut config = minimal_config_with_auto_dispatch(None);
        config.providers.get_mut("kiro").unwrap().prompt_command = Some("{prompt}".to_string());

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, status, auto_mode) VALUES ('p1', 'myapp', '/repo', 'active', 1)",
            [],
        ).unwrap();

        let orch = build_orchestrator_config(&config, &conn).unwrap();

        assert_eq!(orch.projects[0].dispatch_config.prompt_wrapper, None);
        assert_eq!(
            orch.projects[0].dispatch_config.prompt_command,
            Some("{prompt}".to_string()),
        );
    }

    #[test]
    fn list_tasks_populates_subtasks_from_parent_key() {
        use planeai_tasks::model::CreateParams;
        use planeai_tasks::provider::TaskProvider;

        let repo = SqliteRepository::open_in_memory("TST").unwrap();
        // Create parent
        let parent = repo
            .create(CreateParams {
                title: "Parent task".into(),
                ..Default::default()
            })
            .unwrap();
        // Create children pointing to parent
        let child1 = repo
            .create(CreateParams {
                title: "Child one".into(),
                parent_key: Some(parent.key.clone()),
                ..Default::default()
            })
            .unwrap();
        let child2 = repo
            .create(CreateParams {
                title: "Child two".into(),
                parent_key: Some(parent.key.clone()),
                ..Default::default()
            })
            .unwrap();

        let source = SqliteTaskSource::new(repo, vec!["done".into()]);
        let tasks = source.list_tasks().unwrap();

        let parent_task = tasks.iter().find(|t| t.key == parent.key).unwrap();
        assert!(parent_task.subtasks.contains(&child1.key));
        assert!(parent_task.subtasks.contains(&child2.key));

        // Children should have no subtasks
        let c1 = tasks.iter().find(|t| t.key == child1.key).unwrap();
        let c2 = tasks.iter().find(|t| t.key == child2.key).unwrap();
        assert!(c1.subtasks.is_empty());
        assert!(c2.subtasks.is_empty());
    }
}
