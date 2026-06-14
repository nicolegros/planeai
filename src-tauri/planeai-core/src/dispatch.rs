use std::collections::HashSet;
use std::sync::Arc;

use crate::task::{Task, TaskSource};

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
    source: Arc<dyn TaskSource>,
}

impl TaskDispatcher {
    pub fn new(source: Arc<dyn TaskSource>) -> Self {
        Self { source }
    }

    pub async fn fetch_dispatchable_tasks(
        &self,
        claimed: &HashSet<String>,
    ) -> Result<Vec<Task>, DispatchError> {
        let tasks = self
            .source
            .list_tasks()
            .map_err(DispatchError::CommandFailed)?;

        let mut eligible = Vec::new();
        for task in &tasks {
            if claimed.contains(&task.key) {
                continue;
            }
            if self.source.is_terminal(&task.status) {
                continue;
            }
            if !task.subtasks.is_empty() {
                continue;
            }
            if self.has_unresolved_blockers(task, &tasks) {
                continue;
            }
            eligible.push(task.clone());
        }

        eligible.sort_by_key(|t| t.priority);
        Ok(eligible)
    }

    fn has_unresolved_blockers(&self, task: &Task, all_tasks: &[Task]) -> bool {
        for blocker_key in &task.blocked_by {
            let resolved = if let Some(blocker) = all_tasks.iter().find(|t| &t.key == blocker_key) {
                self.source.is_terminal(&blocker.status)
            } else {
                match self.source.get_task(blocker_key) {
                    Ok(blocker) => self.source.is_terminal(&blocker.status),
                    Err(_) => true, // not found = resolved
                }
            };
            if !resolved {
                return true;
            }
        }
        false
    }
}
