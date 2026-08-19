//! Generic, immutable task lifecycle events emitted after PlaneAI persists task changes.

use chrono::{DateTime, Utc};
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
