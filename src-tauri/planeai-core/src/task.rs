use serde::{Deserialize, Serialize};

/// Fixed-contract task structure returned by all task manager CLIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LifecycleHook {
    pub move_to: String,
}

/// Configuration for a task manager's commands.
#[derive(Debug, Clone)]
pub struct TaskManagerConfig {
    pub get_task: String,
    pub list_tasks: String,
    pub move_task: String,
    pub terminal_states: Vec<String>,
    pub on_start: Option<LifecycleHook>,
}
