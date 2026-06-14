//! Legacy external CLI task manager. Kept for Symphony auto_dispatch (see PLA-72).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::config::TaskManager;
use crate::template;

/// Fixed-contract task structure returned by all task manager CLIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskItem {
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
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// Fetch a single task by key.
pub fn get_task(tm: &TaskManager, key: &str, cwd: &Path) -> Result<TaskItem, String> {
    let mut vars = HashMap::new();
    vars.insert("key", key);
    let cmd_str = template::render(&tm.get_task, &vars);
    let output = run_command(&cmd_str, cwd)?;
    serde_json::from_str(&output).map_err(|e| format!("Failed to parse task JSON: {e}"))
}

/// List tasks from the configured command.
pub fn list_tasks(tm: &TaskManager, cwd: &Path) -> Result<Vec<TaskItem>, String> {
    let vars = HashMap::new();
    let cmd_str = template::render(&tm.list_tasks, &vars);
    let output = run_command(&cmd_str, cwd)?;
    serde_json::from_str(&output).map_err(|e| format!("Failed to parse task list JSON: {e}"))
}

/// Move a task to a new status.
pub fn move_task(tm: &TaskManager, key: &str, status: &str, cwd: &Path) -> Result<(), String> {
    let mut vars = HashMap::new();
    vars.insert("key", key);
    vars.insert("status", status);
    let cmd_str = template::render(&tm.move_task, &vars);
    run_command(&cmd_str, cwd)?;
    Ok(())
}

/// List all tasks (all statuses). Falls back to list_tasks if list_all_tasks is not configured.
pub fn list_all_tasks(tm: &TaskManager, cwd: &Path) -> Result<Vec<TaskItem>, String> {
    let cmd_template = tm.list_all_tasks.as_deref().unwrap_or(&tm.list_tasks);
    let vars = HashMap::new();
    let cmd_str = template::render(cmd_template, &vars);
    let output = run_command(&cmd_str, cwd)?;
    serde_json::from_str(&output).map_err(|e| format!("Failed to parse task list JSON: {e}"))
}

/// Create a new task.
pub fn create_task(
    tm: &TaskManager,
    title: &str,
    description: &str,
    priority: i32,
    tags: &[String],
    blocked_by: &[String],
    cwd: &Path,
) -> Result<TaskItem, String> {
    let cmd_template = tm
        .create_task
        .as_deref()
        .ok_or("create_task not configured")?;
    let priority_str = priority.to_string();
    let tags_str = tags.join(",");
    let blocked_by_str = blocked_by.join(",");
    let mut vars = HashMap::new();
    vars.insert("title", title);
    vars.insert("description", description);
    vars.insert("priority", priority_str.as_str());
    vars.insert("tags", tags_str.as_str());
    vars.insert("blocked_by", blocked_by_str.as_str());
    let cmd_str = template::render(cmd_template, &vars);
    let output = run_command(&cmd_str, cwd)?;
    serde_json::from_str(&output).map_err(|e| format!("Failed to parse created task JSON: {e}"))
}

/// Edit an existing task (fetch-merge-interpolate).
/// Only provided fields override the existing values.
#[allow(clippy::too_many_arguments)]
pub fn edit_task(
    tm: &TaskManager,
    key: &str,
    title: Option<&str>,
    description: Option<&str>,
    priority: Option<i32>,
    tags: Option<&[String]>,
    blocked_by: Option<&[String]>,
    cwd: &Path,
) -> Result<TaskItem, String> {
    let cmd_template = tm.edit_task.as_deref().ok_or("edit_task not configured")?;
    // Fetch current task to merge
    let current = get_task(tm, key, cwd)?;
    let merged_title = title.unwrap_or(&current.title);
    let merged_desc = description.unwrap_or(&current.description);
    let merged_priority = priority.unwrap_or(current.priority);
    let merged_tags = tags.map(|t| t.to_vec()).unwrap_or(current.tags.clone());
    let merged_blocked = blocked_by
        .map(|b| b.to_vec())
        .unwrap_or(current.blocked_by.clone());

    let priority_str = merged_priority.to_string();
    let tags_str = merged_tags.join(",");
    let blocked_by_str = merged_blocked.join(",");
    let mut vars = HashMap::new();
    vars.insert("key", key);
    vars.insert("title", merged_title);
    vars.insert("description", merged_desc);
    vars.insert("priority", priority_str.as_str());
    vars.insert("tags", tags_str.as_str());
    vars.insert("blocked_by", blocked_by_str.as_str());
    let cmd_str = template::render(cmd_template, &vars);
    let output = run_command(&cmd_str, cwd)?;
    serde_json::from_str(&output).map_err(|e| format!("Failed to parse edited task JSON: {e}"))
}

fn run_command(cmd_str: &str, cwd: &Path) -> Result<String, String> {
    planeai_core::command::run_command(cmd_str, cwd).map_err(|e| e.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn mock_task_manager(dir: &Path) -> TaskManager {
        // Create mock scripts that return fixed JSON
        let get_script = dir.join("mock_get.sh");
        fs::write(&get_script, "#!/bin/sh\necho '{\"key\":\"KAN-1\",\"title\":\"Fix bug\",\"status\":\"todo\",\"description\":\"A nasty bug\",\"priority\":1,\"blocked_by\":[]}'").unwrap();
        fs::set_permissions(
            &get_script,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let list_script = dir.join("mock_list.sh");
        fs::write(&list_script, "#!/bin/sh\necho '[{\"key\":\"KAN-1\",\"title\":\"Fix bug\",\"status\":\"todo\",\"description\":\"\",\"priority\":1,\"blocked_by\":[]},{\"key\":\"KAN-2\",\"title\":\"Add feature\",\"status\":\"todo\",\"description\":\"\",\"priority\":2,\"blocked_by\":[\"KAN-1\"]}]'").unwrap();
        fs::set_permissions(
            &list_script,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let move_script = dir.join("mock_move.sh");
        fs::write(&move_script, "#!/bin/sh\necho '{}'").unwrap();
        fs::set_permissions(
            &move_script,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        TaskManager {
            get_task: format!("{} {{key}}", get_script.display()),
            move_task: format!("{} {{key}} {{status}}", move_script.display()),
            list_tasks: format!("{}", list_script.display()),
            list_all_tasks: None,
            create_task: None,
            edit_task: None,
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_complete: None,
            on_pr_open: None,
            on_pr_merge: None,
            auto_dispatch: None,
        }
    }

    #[test]
    fn get_task_parses_json_output() {
        let dir = tempdir().unwrap();
        let tm = mock_task_manager(dir.path());

        let task = get_task(&tm, "KAN-1", dir.path()).unwrap();

        assert_eq!(task.key, "KAN-1");
        assert_eq!(task.title, "Fix bug");
        assert_eq!(task.status, "todo");
        assert_eq!(task.description, "A nasty bug");
        assert_eq!(task.priority, 1);
    }

    #[test]
    fn list_tasks_parses_json_array() {
        let dir = tempdir().unwrap();
        let tm = mock_task_manager(dir.path());

        let tasks = list_tasks(&tm, dir.path()).unwrap();

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].key, "KAN-1");
        assert_eq!(tasks[1].key, "KAN-2");
        assert_eq!(tasks[1].blocked_by, vec!["KAN-1"]);
    }

    #[test]
    fn move_task_executes_without_error() {
        let dir = tempdir().unwrap();
        let tm = mock_task_manager(dir.path());

        let result = move_task(&tm, "KAN-1", "in_progress", dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn get_task_returns_error_on_bad_command() {
        let tm = TaskManager {
            get_task: "nonexistent_binary_xyz {key}".to_string(),
            move_task: String::new(),
            list_tasks: String::new(),
            list_all_tasks: None,
            create_task: None,
            edit_task: None,
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_complete: None,
            on_pr_open: None,
            on_pr_merge: None,
            auto_dispatch: None,
        };
        let dir = tempdir().unwrap();
        let result = get_task(&tm, "X-1", dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn get_task_returns_error_on_invalid_json() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("bad.sh");
        fs::write(&script, "#!/bin/sh\necho 'not json'").unwrap();
        fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let tm = TaskManager {
            get_task: format!("{} {{key}}", script.display()),
            move_task: String::new(),
            list_tasks: String::new(),
            list_all_tasks: None,
            create_task: None,
            edit_task: None,
            templates: None,
            on_start: None,
            on_notify: None,
            on_restart: None,
            on_complete: None,
            on_pr_open: None,
            on_pr_merge: None,
            auto_dispatch: None,
        };
        let result = get_task(&tm, "X-1", dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("parse"));
    }
}
