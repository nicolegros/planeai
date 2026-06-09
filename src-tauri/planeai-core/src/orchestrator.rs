use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::AsyncBufReadExt;
use tokio::net::UnixListener;

use crate::dispatch::TaskDispatcher;
use crate::session::{Backend, DispatchConfig, SessionDispatcher};
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

/// The orchestrator loop. Polls tasks, dispatches sessions, listens for stop.
pub struct Orchestrator {
    config: OrchestratorConfig,
    backend: Arc<dyn Backend>,
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig, backend: Arc<dyn Backend>) -> Self {
        Self { config, backend }
    }

    /// Run the orchestrator until a stop command is received on the socket.
    pub async fn run(&self) -> Result<(), String> {
        // Bind the control socket
        let listener = UnixListener::bind(&self.config.socket_path)
            .map_err(|e| format!("failed to bind socket: {e}"))?;

        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(self.config.poll_interval_ms));

        let mut claimed: HashSet<String> = HashSet::new();

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.poll_and_dispatch(&mut claimed).await;
                }
                accept = listener.accept() => {
                    if let Ok((stream, _)) = accept {
                        let reader = tokio::io::BufReader::new(stream);
                        let mut lines = reader.lines();
                        if let Ok(Some(line)) = lines.next_line().await {
                            if line.trim() == "stop" {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }

    async fn poll_and_dispatch(&self, claimed: &mut HashSet<String>) {
        for project in &self.config.projects {
            if claimed.len() >= self.config.max_concurrent {
                break;
            }

            let dispatcher = TaskDispatcher::new(
                &project.task_manager_config,
                &project.project_name,
                std::path::Path::new(&project.project_path),
            );

            let tasks = match dispatcher.fetch_dispatchable_tasks(claimed).await {
                Ok(t) => t,
                Err(_) => continue,
            };

            for task in tasks {
                if claimed.len() >= self.config.max_concurrent {
                    break;
                }

                let session_dispatcher = SessionDispatcher {
                    task_manager_config: project.task_manager_config.clone(),
                    dispatch_config: project.dispatch_config.clone(),
                    project_id: project.project_id.clone(),
                    project_name: project.project_name.clone(),
                    project_path: project.project_path.clone(),
                };

                if let Ok(session) = session_dispatcher.dispatch(&task, self.backend.as_ref()) {
                    claimed.insert(session.task_key);
                }
            }
        }
    }
}
