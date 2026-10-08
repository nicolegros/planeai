//! Generic, immutable task lifecycle events emitted after PlaneAI persists task changes.

use chrono::{DateTime, Utc};
use planeai_tasks::model::{Status, Task, UpdateParams};
use planeai_tasks::provider::{Error as TaskError, TaskProvider};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectIdentity {
    pub project_id: String,
    pub project_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleOrigin {
    Ui,
    Cli,
    Axi,
    SessionHook,
    PrHook,
    Symphony,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusChangeCause {
    Direct,
    AutomaticParentCompletion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskLifecycleEvent {
    ChildAssigned {
        child_key: String,
        parent_key: String,
        is_first_child_assignment: bool,
    },
    StatusChanged {
        task_key: String,
        parent_key: Option<String>,
        previous_status: String,
        new_status: String,
        cause: StatusChangeCause,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLifecycleBatch {
    pub batch_id: String,
    pub occurred_at: DateTime<Utc>,
    pub origin: TaskLifecycleOrigin,
    pub project: ProjectIdentity,
    pub events: Vec<TaskLifecycleEvent>,
}

impl TaskLifecycleBatch {
    pub fn new(
        origin: TaskLifecycleOrigin,
        project_id: impl Into<String>,
        project_prefix: impl Into<String>,
        events: Vec<TaskLifecycleEvent>,
    ) -> Self {
        Self {
            batch_id: Uuid::new_v4().to_string(),
            occurred_at: Utc::now(),
            origin,
            project: ProjectIdentity {
                project_id: project_id.into(),
                project_prefix: project_prefix.into(),
            },
            events,
        }
    }
}

/// Change a task's status and return the ordered lifecycle events caused by the
/// committed change, including automatic parent completion.
pub fn move_task_with_lifecycle(
    provider: &dyn TaskProvider,
    key: &str,
    status: Status,
) -> Result<(Task, Vec<TaskLifecycleEvent>), TaskError> {
    let previous = provider.get(key)?;
    let task = provider.update(
        key,
        UpdateParams {
            status: Some(status),
            ..Default::default()
        },
    )?;
    let mut events = Vec::new();
    if previous.status != task.status {
        events.push(TaskLifecycleEvent::StatusChanged {
            task_key: task.key.clone(),
            parent_key: task.parent_key.clone(),
            previous_status: previous.status.as_str().to_string(),
            new_status: task.status.as_str().to_string(),
            cause: StatusChangeCause::Direct,
        });
    }
    if task.status == Status::Done {
        let parent_before = task
            .parent_key
            .as_deref()
            .and_then(|parent_key| provider.get(parent_key).ok());
        if let Some(parent_key) = planeai_tasks::try_auto_complete_parent(provider, &task) {
            if let (Some(before), Ok(parent)) = (parent_before, provider.get(&parent_key)) {
                events.push(TaskLifecycleEvent::StatusChanged {
                    task_key: parent.key,
                    parent_key: parent.parent_key,
                    previous_status: before.status.as_str().to_string(),
                    new_status: parent.status.as_str().to_string(),
                    cause: StatusChangeCause::AutomaticParentCompletion,
                });
            }
        }
    }
    Ok((task, events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_preserves_event_order_and_project_identity() {
        let batch = TaskLifecycleBatch::new(
            TaskLifecycleOrigin::Ui,
            "project-1",
            "PLA",
            vec![
                TaskLifecycleEvent::StatusChanged {
                    task_key: "PLA-2".into(),
                    parent_key: Some("PLA-1".into()),
                    previous_status: "in_progress".into(),
                    new_status: "done".into(),
                    cause: StatusChangeCause::Direct,
                },
                TaskLifecycleEvent::StatusChanged {
                    task_key: "PLA-1".into(),
                    parent_key: None,
                    previous_status: "todo".into(),
                    new_status: "done".into(),
                    cause: StatusChangeCause::AutomaticParentCompletion,
                },
            ],
        );

        assert_eq!(batch.project.project_id, "project-1");
        assert_eq!(batch.events.len(), 2);
        assert!(matches!(
            batch.events[1],
            TaskLifecycleEvent::StatusChanged {
                cause: StatusChangeCause::AutomaticParentCompletion,
                ..
            }
        ));
    }
}
