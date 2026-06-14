use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::dispatch::TaskDispatcher;
use crate::session::{Backend, DispatchConfig, NewSession, OnStartHook, SessionDispatcher};
use crate::task::TaskSource;

/// Commands the orchestrator can receive via its channel.
pub enum OrchestratorCommand {
    Stop,
    Status {
        reply: tokio::sync::oneshot::Sender<String>,
    },
}

/// Configuration for one project in auto-mode.
#[derive(Clone)]
pub struct AutoProject {
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub task_source: Arc<dyn TaskSource>,
    pub on_start: Option<OnStartHook>,
    pub dispatch_config: DispatchConfig,
}

/// Top-level orchestrator config.
pub struct OrchestratorConfig {
    pub poll_interval_ms: u64,
    pub max_concurrent: usize,
    pub projects: Vec<AutoProject>,
}

/// Tracks a running session and its project context.
struct RunningSession {
    session: NewSession,
    task_source: Arc<dyn TaskSource>,
}

/// The orchestrator loop. Polls tasks, dispatches sessions, listens for commands.
pub struct Orchestrator {
    config: OrchestratorConfig,
    backend: Arc<dyn Backend>,
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig, backend: Arc<dyn Backend>) -> Self {
        Self { config, backend }
    }

    /// Run the orchestrator until cancelled or a Stop command is received.
    pub async fn run(
        &self,
        token: CancellationToken,
        mut commands: mpsc::Receiver<OrchestratorCommand>,
    ) -> Result<(), String> {
        tracing::info!(
            poll_interval_ms = self.config.poll_interval_ms,
            max_concurrent = self.config.max_concurrent,
            projects = self.config.projects.len(),
            "orchestrator starting"
        );

        let mut interval = tokio::time::interval(std::time::Duration::from_millis(
            self.config.poll_interval_ms,
        ));

        // Reattach: load active auto-dispatched sessions from DB
        let mut running: HashMap<String, RunningSession> = HashMap::new();
        if let Ok(sessions) = self.backend.list_active_sessions() {
            tracing::info!(count = sessions.len(), "reattaching active sessions");
            for session in sessions {
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
                            task_source: project.task_source.clone(),
                        },
                    );
                }
            }
        }

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    tracing::info!("orchestrator cancelled");
                    return Ok(());
                }
                _ = interval.tick() => {
                    self.reconcile(&mut running).await;
                    self.dispatch(&mut running).await;
                }
                Some(cmd) = commands.recv() => {
                    match cmd {
                        OrchestratorCommand::Stop => {
                            tracing::info!("orchestrator received stop command");
                            token.cancel();
                            return Ok(());
                        }
                        OrchestratorCommand::Status { reply } => {
                            let sessions: Vec<&str> = running.keys().map(|k| k.as_str()).collect();
                            let json = format!(
                                "{{\"running\":{},\"max_concurrent\":{},\"slots_used\":{}}}",
                                serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_string()),
                                self.config.max_concurrent,
                                running.len()
                            );
                            let _ = reply.send(json);
                        }
                    }
                }
            }
        }
    }

    async fn reconcile(&self, running: &mut HashMap<String, RunningSession>) {
        let mut to_kill = Vec::new();
        for (task_key, entry) in running.iter() {
            if let Ok(task) = entry.task_source.get_task(task_key) {
                if entry.task_source.is_terminal(&task.status) {
                    to_kill.push(task_key.clone());
                }
            }
        }

        for key in to_kill {
            if let Some(entry) = running.remove(&key) {
                tracing::info!(task_key = %key, session_id = %entry.session.id, "killing session — task reached terminal state");
                let _ = self.backend.kill_session(&entry.session);
            }
        }
    }

    async fn dispatch(&self, running: &mut HashMap<String, RunningSession>) {
        let claimed: HashSet<String> = running.keys().cloned().collect();

        for project in &self.config.projects {
            if running.len() >= self.config.max_concurrent {
                break;
            }

            let dispatcher = TaskDispatcher::new(project.task_source.clone());

            let tasks = match dispatcher.fetch_dispatchable_tasks(&claimed).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(project = %project.project_name, error = %e, "failed to fetch dispatchable tasks");
                    continue;
                }
            };

            for task in tasks {
                if running.len() >= self.config.max_concurrent {
                    break;
                }
                if running.contains_key(&task.key) {
                    continue;
                }

                let session_dispatcher = SessionDispatcher {
                    task_source: project.task_source.clone(),
                    on_start: project.on_start.clone(),
                    dispatch_config: self
                        .backend
                        .reload_dispatch_config(&project.dispatch_config.provider)
                        .unwrap_or_else(|| project.dispatch_config.clone()),
                    project_id: project.project_id.clone(),
                    project_name: project.project_name.clone(),
                    project_path: project.project_path.clone(),
                };

                match session_dispatcher.dispatch(&task, self.backend.as_ref()) {
                    Ok(session) => {
                        tracing::info!(
                            task_key = %task.key,
                            session_id = %session.id,
                            project = %project.project_name,
                            "dispatched session for task"
                        );
                        running.insert(
                            session.task_key.clone(),
                            RunningSession {
                                session,
                                task_source: project.task_source.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        tracing::error!(task_key = %task.key, project = %project.project_name, error = %e, "failed to dispatch session");
                    }
                }
            }
        }
    }
}
