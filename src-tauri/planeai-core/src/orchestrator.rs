use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;

use crate::dispatch::TaskDispatcher;
use crate::session::{Backend, DispatchConfig, NewSession, SessionDispatcher};
use crate::task::TaskManagerConfig;

/// Configuration for one project in auto-mode.
#[derive(Debug, Clone)]
pub struct AutoProject {
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub task_manager_config: TaskManagerConfig,
    pub dispatch_config: DispatchConfig,
}

/// Top-level orchestrator config.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub poll_interval_ms: u64,
    pub max_concurrent: usize,
    pub socket_path: PathBuf,
    pub projects: Vec<AutoProject>,
}

/// Tracks a running session and its project context.
struct RunningSession {
    session: NewSession,
    project_path: String,
    task_manager_config: TaskManagerConfig,
}

/// The orchestrator loop. Polls tasks, dispatches sessions, listens for stop.
pub struct Orchestrator {
    config: OrchestratorConfig,
    backend: Arc<dyn Backend>,
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig, backend: Arc<dyn Backend>) -> Self {
        Self { config, backend }
    }

    /// Run the orchestrator until cancelled or a stop command is received on the socket.
    pub async fn run(&self, token: CancellationToken) -> Result<(), String> {
        let _ = std::fs::remove_file(&self.config.socket_path);
        let listener = UnixListener::bind(&self.config.socket_path)
            .map_err(|e| format!("failed to bind socket: {e}"))?;

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(
            self.config.poll_interval_ms,
        ));

        // Reattach: load active auto-dispatched sessions from DB
        let mut running: HashMap<String, RunningSession> = HashMap::new();
        if let Ok(sessions) = self.backend.list_active_sessions() {
            for session in sessions {
                // Find the matching project config for reconciliation
                let project_config = self
                    .config
                    .projects
                    .iter()
                    .find(|p| p.project_id == session.project_id);
                if let Some(project) = project_config {
                    running.insert(
                        session.task_key.clone(),
                        RunningSession {
                            session,
                            project_path: project.project_path.clone(),
                            task_manager_config: project.task_manager_config.clone(),
                        },
                    );
                }
            }
        }

        loop {
            tokio::select! {
                _ = token.cancelled() => return Ok(()),
                _ = interval.tick() => {
                    self.reconcile(&mut running).await;
                    self.dispatch(&mut running).await;
                }
                accept = listener.accept() => {
                    if let Ok((stream, _)) = accept {
                        let mut reader = tokio::io::BufReader::new(stream);
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.is_ok() {
                            match line.trim() {
                                "stop" => {
                                    token.cancel();
                                    return Ok(());
                                }
                                "status" => {
                                    let sessions: Vec<&str> = running.keys().map(|k| k.as_str()).collect();
                                    let json = format!(
                                        "{{\"running\":{},\"max_concurrent\":{},\"slots_used\":{}}}",
                                        serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_string()),
                                        self.config.max_concurrent,
                                        running.len()
                                    );
                                    let mut writer = reader.into_inner();
                                    let _ = writer.write_all(format!("{json}\n").as_bytes()).await;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    async fn reconcile(&self, running: &mut HashMap<String, RunningSession>) {
        let mut to_kill = Vec::new();

        for (task_key, entry) in running.iter() {
            let status =
                self.get_task_status(&entry.task_manager_config, task_key, &entry.project_path);
            if let Some(status) = status {
                if entry
                    .task_manager_config
                    .terminal_states
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&status))
                {
                    to_kill.push(task_key.clone());
                }
            }
        }

        for key in to_kill {
            if let Some(entry) = running.remove(&key) {
                let _ = self.backend.kill_session(&entry.session);
            }
        }
    }

    fn get_task_status(
        &self,
        config: &TaskManagerConfig,
        key: &str,
        project_path: &str,
    ) -> Option<String> {
        use std::collections::HashMap as StdMap;
        let mut vars = StdMap::new();
        vars.insert("key", key);
        let cmd_str = crate::template::render(&config.get_task, &vars);
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }
        let output = std::process::Command::new(parts[0])
            .args(&parts[1..])
            .current_dir(project_path)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let task: crate::task::Task = serde_json::from_str(&stdout).ok()?;
        Some(task.status)
    }

    async fn dispatch(&self, running: &mut HashMap<String, RunningSession>) {
        let claimed: HashSet<String> = running.keys().cloned().collect();

        for project in &self.config.projects {
            if running.len() >= self.config.max_concurrent {
                break;
            }

            let dispatcher = TaskDispatcher::new(
                &project.task_manager_config,
                &project.project_name,
                Path::new(&project.project_path),
            );

            let tasks = match dispatcher.fetch_dispatchable_tasks(&claimed).await {
                Ok(t) => t,
                Err(_) => continue,
            };

            for task in tasks {
                if running.len() >= self.config.max_concurrent {
                    break;
                }
                if running.contains_key(&task.key) {
                    continue;
                }

                let session_dispatcher = SessionDispatcher {
                    task_manager_config: project.task_manager_config.clone(),
                    dispatch_config: project.dispatch_config.clone(),
                    project_id: project.project_id.clone(),
                    project_name: project.project_name.clone(),
                    project_path: project.project_path.clone(),
                };

                if let Ok(session) = session_dispatcher.dispatch(&task, self.backend.as_ref()) {
                    running.insert(
                        session.task_key.clone(),
                        RunningSession {
                            session,
                            project_path: project.project_path.clone(),
                            task_manager_config: project.task_manager_config.clone(),
                        },
                    );
                }
            }
        }
    }
}
