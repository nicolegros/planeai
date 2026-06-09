use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

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

fn run_command(cmd_str: &str, cwd: &Path) -> Result<String, String> {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }
    let resolved = crate::command::resolve(parts[0]);
    let output = Command::new(&resolved)
        .args(&parts[1..])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("Failed to execute '{}': {e}", resolved))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Command failed ({}): {}",
            output.status,
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
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
