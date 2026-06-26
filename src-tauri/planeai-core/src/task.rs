use serde::{Deserialize, Serialize};

/// Fixed-contract task structure used by the orchestrator.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    pub parent_key: Option<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub subtasks: Vec<String>,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
}

fn default_base_branch() -> String {
    "main".to_string()
}

/// Abstraction over task storage. Replaces CLI-based TaskManagerConfig.
pub trait TaskSource: Send + Sync {
    /// List all non-terminal tasks for the project.
    fn list_tasks(&self) -> Result<Vec<Task>, String>;
    /// Get a single task by key.
    fn get_task(&self, key: &str) -> Result<Task, String>;
    /// Move a task to a new status.
    fn move_task(&self, key: &str, status: &str) -> Result<(), String>;
    /// Returns true if `status` is a terminal state (done, cancelled, etc.)
    fn is_terminal(&self, status: &str) -> bool;
}
