use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::command::run_command;
use crate::task::{Task, TaskManagerConfig};
use crate::template;

#[derive(Debug)]
pub enum DispatchError {
    CommandFailed(String),
    ParseError(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandFailed(msg) => write!(f, "command failed: {msg}"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

pub struct TaskDispatcher {
    config: TaskManagerConfig,
    project: String,
    cwd: std::path::PathBuf,
}

impl TaskDispatcher {
    pub fn new(config: &TaskManagerConfig, project_name: &str, cwd: &Path) -> Self {
        Self {
            config: config.clone(),
            project: project_name.to_string(),
            cwd: cwd.to_path_buf(),
        }
    }

    pub async fn fetch_dispatchable_tasks(
        &self,
        claimed: &HashSet<String>,
    ) -> Result<Vec<Task>, DispatchError> {
        let tasks = self.run_list_tasks().await?;

        let mut eligible = Vec::new();
        for task in &tasks {
            if claimed.contains(&task.key) {
                continue;
            }
            if self.is_terminal(&task.status) {
                continue;
            }
            if !task.subtasks.is_empty() {
                continue;
            }
            if self.has_unresolved_blockers(task, &tasks).await? {
                continue;
            }
            eligible.push(task.clone());
        }

        eligible.sort_by_key(|t| t.priority);
        Ok(eligible)
    }

    async fn run_list_tasks(&self) -> Result<Vec<Task>, DispatchError> {
        let mut vars = HashMap::new();
        vars.insert("project", self.project.as_str());
        let cmd_str = template::render(&self.config.list_tasks, &vars);
        let output = self.run_cmd(&cmd_str)?;
        serde_json::from_str(&output)
            .map_err(|e| DispatchError::ParseError(format!("list_tasks: {e}")))
    }

    async fn run_get_task(&self, key: &str) -> Result<Task, DispatchError> {
        let mut vars = HashMap::new();
        vars.insert("key", key);
        let cmd_str = template::render(&self.config.get_task, &vars);
        let output = self.run_cmd(&cmd_str)?;
        serde_json::from_str(&output)
            .map_err(|e| DispatchError::ParseError(format!("get_task({key}): {e}")))
    }

    async fn has_unresolved_blockers(
        &self,
        task: &Task,
        all_tasks: &[Task],
    ) -> Result<bool, DispatchError> {
        for blocker_key in &task.blocked_by {
            let resolved = if let Some(blocker) = all_tasks.iter().find(|t| &t.key == blocker_key) {
                self.is_terminal(&blocker.status)
            } else {
                match self.run_get_task(blocker_key).await {
                    Ok(blocker) => self.is_terminal(&blocker.status),
                    Err(_) => true, // not found = resolved
                }
            };
            if !resolved {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn is_terminal(&self, status: &str) -> bool {
        self.config
            .terminal_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case(status))
    }

    fn run_cmd(&self, cmd_str: &str) -> Result<String, DispatchError> {
        run_command(cmd_str, &self.cwd).map_err(|e| DispatchError::CommandFailed(e.to_string()))
    }
}
